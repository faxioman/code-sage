
use crate::{Error, Result};
use super::{VectorDatabase, VectorDocument, SearchResult};
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use std::collections::HashMap;
use usearch::Index;
use usearch::ScalarKind;
use usearch::MetricKind;
use sha2::{Sha256, Digest};

pub struct USearchDatabase {
    index: Index,
    path: PathBuf,
    dimension: usize,
    data_dir: PathBuf,
    id_map: HashMap<String, u64>,
    reverse_id_map: HashMap<u64, String>,
    next_id: u64,
}

impl USearchDatabase {
    pub fn new(path: PathBuf, dimension: usize, data_dir: PathBuf) -> Result<Self> {
        let index = Index::new(&usearch::IndexOptions {
            dimensions: dimension,
            metric: MetricKind::Cos,
            quantization: ScalarKind::F32,
            connectivity: 16,
            expansion_add: 128,
            expansion_search: 64,
            multi: false,
        }).map_err(|e| Error::VectorDb(format!("Failed to create index: {e:?}")))?;
        
        Ok(Self {
            index,
            path,
            dimension,
            data_dir,
            id_map: HashMap::new(),
            reverse_id_map: HashMap::new(),
            next_id: 0,
        })
    }
    
    pub fn from_file(path: PathBuf, data_dir: PathBuf) -> Result<Self> {
        let index = Index::new(&usearch::IndexOptions {
            dimensions: 1536,
            metric: MetricKind::Cos,
            quantization: ScalarKind::F32,
            connectivity: 16,
            expansion_add: 128,
            expansion_search: 64,
            multi: false,
        }).map_err(|e| Error::VectorDb(format!("Failed to create index: {e:?}")))?;
        
        if path.exists() {
            index.load(&path.to_string_lossy())
                .map_err(|e| Error::VectorDb(format!("Failed to load index: {e:?}")))?;
        }
        
        let dimension = index.dimensions();
        
        let mut db = Self {
            index,
            path: path.clone(),
            dimension,
            data_dir,
            id_map: HashMap::new(),
            reverse_id_map: HashMap::new(),
            next_id: 0,
        };
        
        db.load_mappings_sync()?;
        
        Ok(db)
    }
    
    fn load_mappings_sync(&mut self) -> Result<()> {
        let mappings_path = self.path.with_extension("mappings.json");
        if mappings_path.exists() {
            let mappings_str = std::fs::read_to_string(mappings_path)?;
            let mappings: serde_json::Value = serde_json::from_str(&mappings_str)?;
            
            if let Some(id_map_obj) = mappings.get("id_map").and_then(|v| v.as_object()) {
                for (key, value) in id_map_obj {
                    if let Some(id) = value.as_u64() {
                        self.id_map.insert(key.clone(), id);
                        self.reverse_id_map.insert(id, key.clone());
                    }
                }
            }
            
            if let Some(next_id) = mappings.get("next_id").and_then(|v| v.as_u64()) {
                self.next_id = next_id;
            }
        }
        
        Ok(())
    }
    
    fn get_or_create_internal_id(&mut self, string_id: &str) -> u64 {
        if let Some(&id) = self.id_map.get(string_id) {
            id
        } else {
            let id = self.next_id;
            self.id_map.insert(string_id.to_string(), id);
            self.reverse_id_map.insert(id, string_id.to_string());
            self.next_id += 1;
            id
        }
    }
}

#[async_trait]
impl VectorDatabase for USearchDatabase {
    async fn insert(&mut self, documents: Vec<VectorDocument>) -> Result<()> {
        let current_size = self.index.size();
        let needed_capacity = current_size + documents.len();
        
        self.index
            .reserve(needed_capacity)
            .map_err(|e| Error::VectorDb(format!("Failed to reserve capacity: {e:?}")))?;
        
        
        for doc in documents.iter() {
            if doc.vector.len() != self.dimension {
                return Err(Error::VectorDb(format!(
                    "Vector dimension mismatch: expected {}, got {}",
                    self.dimension,
                    doc.vector.len()
                )));
            }
            
            let internal_id = self.get_or_create_internal_id(&doc.id);
            
            self.index
                .add(internal_id, &doc.vector)
                .map_err(|e| Error::VectorDb(format!("Failed to add vector: {e:?}")))?;
                
        }
        Ok(())
    }
    
    async fn search(&self, query_vector: &[f32], top_k: usize) -> Result<Vec<SearchResult>> {
        if query_vector.len() != self.dimension {
            return Err(Error::VectorDb(format!(
                "Query vector dimension mismatch: expected {}, got {}",
                self.dimension,
                query_vector.len()
            )));
        }
        
        let results = self.index
            .search(query_vector, top_k)
            .map_err(|e| Error::VectorDb(format!("Search failed: {e:?}")))?;
        
        let mut search_results = Vec::new();
        
        for match_result in results.keys.iter().zip(results.distances.iter()) {
            let (internal_id, distance) = match_result;
            
            if let Some(string_id) = self.reverse_id_map.get(internal_id) {
                let score = 1.0 - distance;
                
                search_results.push(SearchResult {
                    id: string_id.clone(),
                    score,
                });
            }
        }
        
        Ok(search_results)
    }
    
    async fn delete(&mut self, ids: &[String]) -> Result<()> {
        for id in ids {
            if let Some(&internal_id) = self.id_map.get(id) {
                self.index
                    .remove(internal_id)
                    .map_err(|e| Error::VectorDb(format!("Failed to remove vector: {e:?}")))?;
                
                self.id_map.remove(id);
                self.reverse_id_map.remove(&internal_id);
            }
        }
        
        Ok(())
    }
    
    async fn count(&self) -> Result<usize> {
        Ok(self.index.size())
    }
    
    async fn save(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        
        self.index
            .save(&self.path.to_string_lossy())
            .map_err(|e| Error::VectorDb(format!("Failed to save index: {e:?}")))?;
        
        let mappings_path = self.path.with_extension("mappings.json");
        let mappings = serde_json::json!({
            "id_map": self.id_map,
            "next_id": self.next_id,
        });
        
        std::fs::write(mappings_path, serde_json::to_string_pretty(&mappings)?)?;
        
        Ok(())
    }
    
    async fn load(&mut self) -> Result<()> {
        if !self.path.exists() {
            return Err(Error::VectorDb("Index file does not exist".to_string()));
        }
        
        self.index
            .load(&self.path.to_string_lossy())
            .map_err(|e| Error::VectorDb(format!("Failed to load index: {e:?}")))?;
        
        self.id_map.clear();
        self.reverse_id_map.clear();
        self.next_id = 0;
        
        let mappings_path = self.path.with_extension("mappings.json");
        if mappings_path.exists() {
            let mappings_str = std::fs::read_to_string(mappings_path)?;
            let mappings: serde_json::Value = serde_json::from_str(&mappings_str)?;
            
            if let Some(id_map_obj) = mappings.get("id_map").and_then(|v| v.as_object()) {
                for (key, value) in id_map_obj {
                    if let Some(id) = value.as_u64() {
                        self.id_map.insert(key.clone(), id);
                        self.reverse_id_map.insert(id, key.clone());
                    }
                }
            }
            
            if let Some(next_id) = mappings.get("next_id").and_then(|v| v.as_u64()) {
                self.next_id = next_id;
            }
        }
        
        Ok(())
    }
    
    async fn has_index(&self, codebase_path: &Path) -> Result<bool> {
        let index_path = Self::get_index_path_for_codebase(codebase_path, &self.data_dir);
        Ok(index_path.exists())
    }
    
    async fn delete_index(&mut self, codebase_path: &Path) -> Result<()> {
        let index_path = Self::get_index_path_for_codebase(codebase_path, &self.data_dir);
        
        if let Some(index_dir) = index_path.parent() {
            if index_dir.exists() {
                std::fs::remove_dir_all(index_dir)?;
            }
        }
        
        self.id_map.clear();
        self.reverse_id_map.clear();
        self.next_id = 0;
        
        Ok(())
    }
}

impl USearchDatabase {
    fn get_index_path_for_codebase(codebase_path: &Path, data_dir: &Path) -> PathBuf {
        let mut hasher = Sha256::new();
        hasher.update(codebase_path.to_string_lossy().as_bytes());
        let hash = format!("{:x}", hasher.finalize());
        
        data_dir
            .join("vectors")
            .join(&hash[..16])
            .join("index.usearch")
    }
    
    pub fn for_codebase(codebase_path: &Path, dimension: usize, data_dir: &Path) -> Result<Self> {
        let index_path = Self::get_index_path_for_codebase(codebase_path, data_dir);
        
        if index_path.exists() {
            Self::from_file(index_path, data_dir.to_path_buf())
        } else {
            Self::new(index_path, dimension, data_dir.to_path_buf())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    
    #[tokio::test]
    async fn test_insert_and_search() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.usearch");
        let data_dir = dir.path().to_path_buf();
        
        let mut db = USearchDatabase::new(path, 128, data_dir).unwrap();
        
        let docs = vec![
            VectorDocument {
                id: "doc1".to_string(),
                vector: vec![1.0; 128],
            },
            VectorDocument {
                id: "doc2".to_string(),
                vector: {
                    let mut v = vec![0.0; 128];
                    v[0] = 1.0;
                    v
                },
            },
        ];
        
        db.insert(docs).await.unwrap();
        
        let query = vec![1.0; 128];
        let results = db.search(&query, 2).await.unwrap();
        
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].id, "doc1");
        assert!(results[0].score > results[1].score);
    }
    
    #[tokio::test]
    async fn test_has_and_delete_index() {
        let dir = tempdir().unwrap();
        let codebase_path = dir.path().join("codebase");
        let data_dir = dir.path().to_path_buf();
        
        let mut db = USearchDatabase::for_codebase(&codebase_path, 128, &data_dir).unwrap();
        
        assert!(!db.has_index(&codebase_path).await.unwrap());
        let docs = vec![VectorDocument {
            id: "test".to_string(),
            vector: vec![1.0; 128],
        }];
        db.insert(docs).await.unwrap();
        db.save().await.unwrap();
        
        assert!(db.has_index(&codebase_path).await.unwrap());
        db.delete_index(&codebase_path).await.unwrap();
        assert!(!db.has_index(&codebase_path).await.unwrap());
    }
    
    #[tokio::test]
    async fn test_save_and_load() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("persistent.usearch");
        let data_dir = dir.path().to_path_buf();
        
        {
            let mut db = USearchDatabase::new(path.clone(), 128, data_dir.clone()).unwrap();
            let docs = vec![VectorDocument {
                id: "persistent_doc".to_string(),
                vector: vec![1.0; 128],
            }];
            db.insert(docs).await.unwrap();
            db.save().await.unwrap();
        }
        
        {
            let mut db = USearchDatabase::from_file(path, data_dir).unwrap();
            db.load().await.unwrap();
            
            assert_eq!(db.count().await.unwrap(), 1);
            
            let query = vec![1.0; 128];
            let results = db.search(&query, 1).await.unwrap();
            assert_eq!(results[0].id, "persistent_doc");
        }
    }
}
