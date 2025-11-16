//! Vector database module using USearch

pub mod usearch_db;

use crate::Result;
use async_trait::async_trait;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct VectorDocument {
    pub id: String,
    pub vector: Vec<f32>,
}

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub id: String,
    pub score: f32,
}

#[async_trait]
pub trait VectorDatabase: Send + Sync {
    /// Insert vectors into the database
    async fn insert(&mut self, documents: Vec<VectorDocument>) -> Result<()>;
    
    /// Search for similar vectors
    async fn search(&self, query_vector: &[f32], top_k: usize) -> Result<Vec<SearchResult>>;
    
    /// Delete vectors by IDs
    async fn delete(&mut self, ids: &[String]) -> Result<()>;
    
    /// Check if index exists for a codebase
    async fn has_index(&self, codebase_path: &Path) -> Result<bool>;
    
    /// Delete entire index for a codebase
    async fn delete_index(&mut self, codebase_path: &Path) -> Result<()>;
    
    async fn insert_batch(
        &mut self,
        _codebase_path: &Path,
        chunks: &[crate::types::CodeChunk],
        embeddings: &[Vec<f32>],
    ) -> Result<()> {
        const STORAGE_BATCH_SIZE: usize = 50;
        
        for (i, (chunk_batch, embedding_batch)) in chunks
            .chunks(STORAGE_BATCH_SIZE)
            .zip(embeddings.chunks(STORAGE_BATCH_SIZE))
            .enumerate()
        {
            let documents: Vec<VectorDocument> = chunk_batch
                .iter()
                .zip(embedding_batch.iter())
                .map(|(chunk, embedding)| VectorDocument {
                    id: chunk.id.clone(),
                    vector: embedding.clone(),
                })
                .collect();
            
            tracing::info!("[VECTOR-DB] Inserting batch {} ({} vectors)", i + 1, documents.len());
            self.insert(documents).await?;
        }
        
        Ok(())
    }
    
    async fn search_codebase(
        &self,
        _codebase_path: &Path,
        query_vector: &[f32],
        top_k: usize,
    ) -> Result<Vec<SearchResult>> {
        self.search(query_vector, top_k).await
    }
    
    /// Get total number of vectors
    async fn count(&self) -> Result<usize>;
    
    /// Save index to disk
    async fn save(&self) -> Result<()>;
    
    /// Load index from disk
    async fn load(&mut self) -> Result<()>;
}

pub use usearch_db::USearchDatabase;
