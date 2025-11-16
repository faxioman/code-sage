//! BM25 full-text search using Tantivy

use crate::{Error, Result};
use super::{BM25Document, BM25Result};
use std::path::{Path, PathBuf};
use tantivy::{
    Index, IndexWriter, IndexReader,
    schema::*,
    query::QueryParser,
    collector::TopDocs,
    TantivyDocument,
};

pub struct BM25Search {
    index: Index,
    reader: IndexReader,
    writer: IndexWriter,
    id_field: Field,
    content_field: Field,
    file_path_field: Field,
    start_line_field: Field,
    end_line_field: Field,
    data_dir: PathBuf, // Needed for computing delete paths
}

impl BM25Search {
    pub fn new(index_dir: &Path, data_dir: PathBuf) -> Result<Self> {
        // Create schema
        let mut schema_builder = Schema::builder();
        
        let id_field = schema_builder.add_text_field("id", STRING | STORED);
        let content_field = schema_builder.add_text_field("content", TEXT);
        let file_path_field = schema_builder.add_text_field("file_path", STRING | STORED);
        let start_line_field = schema_builder.add_u64_field("start_line", STORED);
        let end_line_field = schema_builder.add_u64_field("end_line", STORED);
        
        let schema = schema_builder.build();
        
        // Create or open index
        std::fs::create_dir_all(index_dir)?;
        
        let index = Index::create_in_dir(index_dir, schema.clone())
            .or_else(|_| Index::open_in_dir(index_dir))
            .map_err(|e| Error::FullText(format!("Failed to create/open index: {e}")))?;
        
        let reader = index.reader()
            .map_err(|e| Error::FullText(format!("Failed to create reader: {e}")))?;
        
        let writer = index.writer(50_000_000)
            .map_err(|e| Error::FullText(format!("Failed to create writer: {e}")))?;
        
        Ok(Self {
            index,
            reader,
            writer,
            id_field,
            content_field,
            file_path_field,
            start_line_field,
            end_line_field,
            data_dir,
        })
    }
    
    pub fn insert(&mut self, documents: Vec<BM25Document>) -> Result<()> {
        for doc in documents {
            let mut tantivy_doc = TantivyDocument::default();
            
            tantivy_doc.add_text(self.id_field, &doc.id);
            tantivy_doc.add_text(self.content_field, &doc.content);
            tantivy_doc.add_text(self.file_path_field, &doc.file_path);
            tantivy_doc.add_u64(self.start_line_field, doc.start_line);
            tantivy_doc.add_u64(self.end_line_field, doc.end_line);
            
            self.writer.add_document(tantivy_doc)
                .map_err(|e| Error::FullText(format!("Failed to add document: {e}")))?;
        }
        
        self.writer.commit()
            .map_err(|e| Error::FullText(format!("Failed to commit: {e}")))?;
        
        self.reader.reload()
            .map_err(|e| Error::FullText(format!("Failed to reload: {e}")))?;
        
        Ok(())
    }
    
    pub fn search(&self, query_text: &str, top_k: usize) -> Result<Vec<BM25Result>> {
        let searcher = self.reader.searcher();
        
        let query_parser = QueryParser::for_index(&self.index, vec![self.content_field]);
        let query = query_parser.parse_query(query_text)
            .map_err(|e| Error::FullText(format!("Failed to parse query: {e}")))?;
        
        let top_docs = searcher.search(&query, &TopDocs::with_limit(top_k))
            .map_err(|e| Error::FullText(format!("Search failed: {e}")))?;
        
        let mut results = Vec::new();
        
        for (_score, doc_address) in top_docs {
            let retrieved_doc: TantivyDocument = searcher.doc(doc_address)
                .map_err(|e| Error::FullText(format!("Failed to retrieve doc: {e}")))?;
            
            if let Some(id_value) = retrieved_doc.get_first(self.id_field) {
                if let Some(id) = id_value.as_str() {
                    let normalized_score = _score.clamp(0.0, 1.0);
                    
                    results.push(BM25Result {
                        id: id.to_string(),
                        score: normalized_score,
                    });
                }
            }
        }
        
        Ok(results)
    }
    
    pub fn delete(&mut self, ids: &[String]) -> Result<()> {
        for id in ids {
            let term = tantivy::Term::from_field_text(self.id_field, id);
            let query = tantivy::query::TermQuery::new(term, tantivy::schema::IndexRecordOption::Basic);
            let _ = self.writer.delete_query(Box::new(query));
        }
        
        self.writer.commit()
            .map_err(|e| Error::FullText(format!("Failed to commit deletions: {e}")))?;
        
        self.reader.reload()
            .map_err(|e| Error::FullText(format!("Failed to reload: {e}")))?;
        
        Ok(())
    }
    
    pub fn count(&self) -> Result<usize> {
        let searcher = self.reader.searcher();
        Ok(searcher.num_docs() as usize)
    }
    
    pub fn save(&self) -> Result<()> {
        Ok(())
    }
    
    pub fn load(&mut self) -> Result<()> {
        self.reader.reload()
            .map_err(|e| Error::FullText(format!("Failed to reload: {e}")))?;
        Ok(())
    }
    
    /// Check if index exists for a codebase
    pub async fn has_index(&self, _codebase_path: &Path) -> Result<bool> {
        Ok(self.count()? > 0)
    }
    
    /// Delete entire index for a codebase
    pub async fn delete_index(&mut self, codebase_path: &Path) -> Result<()> {
        let index_dir = Self::get_index_path_for_codebase(codebase_path, &self.data_dir);
        
        if index_dir.exists() {
            std::fs::remove_dir_all(&index_dir)?;
        }
        
        Ok(())
    }
    
    pub async fn insert_batch(
        &mut self,
        _codebase_path: &Path,
        chunks: &[crate::types::CodeChunk],
    ) -> Result<()> {
        const STORAGE_BATCH_SIZE: usize = 50;
        
        for (i, chunk_batch) in chunks.chunks(STORAGE_BATCH_SIZE).enumerate() {
            let documents: Vec<BM25Document> = chunk_batch
                .iter()
                .map(|chunk| BM25Document {
                    id: chunk.id.clone(),
                    content: chunk.content.clone(),
                    file_path: chunk.relative_path.clone(),
                    start_line: chunk.start_line as u64,
                    end_line: chunk.end_line as u64,
                })
                .collect();
            
            tracing::info!("[BM25] Inserting batch {} ({} documents)", i + 1, documents.len());
            self.insert(documents)?;
        }
        
        Ok(())
    }
    
    pub async fn search_codebase(
        &self,
        _codebase_path: &Path,
        query_text: &str,
        top_k: usize,
    ) -> Result<Vec<BM25Result>> {
        self.search(query_text, top_k)
    }
    
    fn get_index_path_for_codebase(codebase_path: &Path, data_dir: &Path) -> PathBuf {
        use sha2::{Sha256, Digest};
        
        let mut hasher = Sha256::new();
        hasher.update(codebase_path.to_string_lossy().as_bytes());
        let hash = format!("{:x}", hasher.finalize());
        
        data_dir
            .join("fulltext")
            .join(&hash[..16])
    }
    
    pub fn for_codebase(codebase_path: &Path, data_dir: &Path) -> Result<Self> {
        let index_dir = Self::get_index_path_for_codebase(codebase_path, data_dir);
        Self::new(&index_dir, data_dir.to_path_buf())
    }
}
