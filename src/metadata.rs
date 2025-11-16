//! Metadata storage using Sled embedded database
//!
//! Stores chunk metadata per codebase for fast lookup during search

use crate::{Result, Error};
use crate::types::CodeChunk;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use sha2::{Sha256, Digest};

/// Metadata store using Sled
pub struct MetadataStore {
    db: sled::Db,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredMetadata {
    pub content: String,
    pub file_path: PathBuf,
    pub relative_path: String,
    pub start_line: usize,
    pub end_line: usize,
    pub language: String,
    pub file_extension: String,
    pub chunk_index: usize,
    pub hash: String,
}

impl From<&CodeChunk> for StoredMetadata {
    fn from(chunk: &CodeChunk) -> Self {
        Self {
            content: chunk.content.clone(),
            file_path: chunk.file_path.clone(),
            relative_path: chunk.relative_path.clone(),
            start_line: chunk.start_line,
            end_line: chunk.end_line,
            language: chunk.language.clone(),
            file_extension: chunk.metadata.file_extension.clone(),
            chunk_index: chunk.metadata.chunk_index,
            hash: chunk.metadata.hash.clone(),
        }
    }
}

impl MetadataStore {
    fn get_db_path_for_codebase(codebase_path: &Path, data_dir: &Path) -> PathBuf {
        let mut hasher = Sha256::new();
        hasher.update(codebase_path.to_string_lossy().as_bytes());
        let hash = format!("{:x}", hasher.finalize());
        
        data_dir
            .join("metadata")
            .join(&hash[..16])
    }
    
    /// Create or open metadata store for a specific codebase
    pub fn for_codebase(codebase_path: &Path, data_dir: &Path) -> Result<Self> {
        let db_path = Self::get_db_path_for_codebase(codebase_path, data_dir);
        
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        
        let db = sled::open(&db_path)
            .map_err(|e| Error::Io(std::io::Error::other(
                format!("Failed to open Sled DB: {e}")
            )))?;
        
        Ok(Self { db })
    }
    
    /// Store metadata for a chunk
    pub fn insert(&self, chunk_id: &str, metadata: &StoredMetadata) -> Result<()> {
        let value = bincode::serde::encode_to_vec(metadata, bincode::config::standard())
            .map_err(|e| Error::Io(std::io::Error::other(
                format!("Failed to serialize metadata: {e}")
            )))?;
        
        self.db.insert(chunk_id.as_bytes(), value)
            .map_err(|e| Error::Io(std::io::Error::other(
                format!("Failed to insert metadata: {e}")
            )))?;
        
        Ok(())
    }
    
    /// Store metadata for multiple chunks (batch)
    pub fn insert_batch(&self, chunks: &[CodeChunk]) -> Result<()> {
        let mut batch = sled::Batch::default();
        
        for chunk in chunks {
            let metadata = StoredMetadata::from(chunk);
            let value = bincode::serde::encode_to_vec(&metadata, bincode::config::standard())
                .map_err(|e| Error::Io(std::io::Error::other(
                    format!("Failed to serialize metadata: {e}")
                )))?;
            
            batch.insert(chunk.id.as_bytes(), value);
        }
        
        self.db.apply_batch(batch)
            .map_err(|e| Error::Io(std::io::Error::other(
                format!("Failed to apply batch: {e}")
            )))?;
        
        Ok(())
    }
    
    /// Get metadata for a chunk
    pub fn get(&self, chunk_id: &str) -> Result<Option<StoredMetadata>> {
        let value = self.db.get(chunk_id.as_bytes())
            .map_err(|e| Error::Io(std::io::Error::other(
                format!("Failed to get metadata: {e}")
            )))?;
        
        match value {
            Some(bytes) => {
                let (metadata, _len) = bincode::serde::decode_from_slice(&bytes, bincode::config::standard())
                    .map_err(|e| Error::Io(std::io::Error::other(
                        format!("Failed to deserialize metadata: {e}")
                    )))?;
                Ok(Some(metadata))
            }
            None => Ok(None),
        }
    }
    
    /// Get metadata for multiple chunks (batch)
    pub fn get_batch(&self, chunk_ids: &[String]) -> Result<Vec<Option<StoredMetadata>>> {
        let mut results = Vec::with_capacity(chunk_ids.len());
        
        for id in chunk_ids {
            results.push(self.get(id)?);
        }
        
        Ok(results)
    }
    
    /// Delete metadata for a chunk
    pub fn delete(&self, chunk_id: &str) -> Result<()> {
        self.db.remove(chunk_id.as_bytes())
            .map_err(|e| Error::Io(std::io::Error::other(
                format!("Failed to delete metadata: {e}")
            )))?;
        Ok(())
    }
    
    /// Clear all metadata for this codebase
    pub fn clear(&self) -> Result<()> {
        self.db.clear()
            .map_err(|e| Error::Io(std::io::Error::other(
                format!("Failed to clear metadata: {e}")
            )))?;
        Ok(())
    }
    
    /// Get count of stored chunks
    pub fn count(&self) -> usize {
        self.db.len()
    }
    
    /// Iterate over all stored metadata
    pub fn iter(&self) -> impl Iterator<Item = (String, StoredMetadata)> + '_ {
        self.db.iter().filter_map(|result| {
            match result {
                Ok((key, value)) => {
                    let chunk_id = String::from_utf8(key.to_vec()).ok()?;
                    let (metadata, _len) = bincode::serde::decode_from_slice(&value, bincode::config::standard()).ok()?;
                    Some((chunk_id, metadata))
                }
                Err(_) => None,
            }
        })
    }
    
    /// Check if metadata exists for a codebase
    pub fn exists(codebase_path: &Path, data_dir: &Path) -> bool {
        let db_path = Self::get_db_path_for_codebase(codebase_path, data_dir);
        db_path.exists()
    }
    
    /// Delete entire metadata store for a codebase
    pub fn delete_for_codebase(codebase_path: &Path, data_dir: &Path) -> Result<()> {
        let db_path = Self::get_db_path_for_codebase(codebase_path, data_dir);
        
        if db_path.exists() {
            std::fs::remove_dir_all(&db_path)?;
        }
        
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ChunkMetadata;
    use tempfile::tempdir;
    
    #[test]
    fn test_metadata_store() {
        let dir = tempdir().unwrap();
        let codebase_path = dir.path().join("test_codebase");
        let data_dir = dir.path().to_path_buf();
        
        let store = MetadataStore::for_codebase(&codebase_path, &data_dir).unwrap();
        
        let metadata = StoredMetadata {
            content: "fn test() {}".to_string(),
            file_path: PathBuf::from("/test/file.rs"),
            relative_path: "file.rs".to_string(),
            start_line: 10,
            end_line: 20,
            language: "rust".to_string(),
            file_extension: ".rs".to_string(),
            chunk_index: 0,
            hash: "abc123".to_string(),
        };
        
        store.insert("chunk_1", &metadata).unwrap();
        assert_eq!(store.count(), 1);
        
        let retrieved = store.get("chunk_1").unwrap().unwrap();
        assert_eq!(retrieved.relative_path, "file.rs");
        assert_eq!(retrieved.start_line, 10);
        
        store.delete("chunk_1").unwrap();
        assert_eq!(store.count(), 0);
        assert!(store.get("chunk_1").unwrap().is_none());
    }
    
    #[test]
    fn test_metadata_batch() {
        let dir = tempdir().unwrap();
        let codebase_path = dir.path().join("test_codebase");
        let data_dir = dir.path().to_path_buf();
        
        let store = MetadataStore::for_codebase(&codebase_path, &data_dir).unwrap();
        
        let chunks: Vec<CodeChunk> = (0..5).map(|i| {
            CodeChunk {
                id: format!("chunk_{i}"),
                content: format!("content {i}"),
                file_path: PathBuf::from(format!("/test/file{i}.rs")),
                relative_path: format!("file{i}.rs"),
                start_line: i * 10,
                end_line: i * 10 + 10,
                language: "rust".to_string(),
                metadata: ChunkMetadata {
                    file_extension: ".rs".to_string(),
                    chunk_index: i,
                    hash: format!("hash{i}"),
                },
            }
        }).collect();
        
        store.insert_batch(&chunks).unwrap();
        assert_eq!(store.count(), 5);
        
        let ids: Vec<String> = (0..5).map(|i| format!("chunk_{i}")).collect();
        let results = store.get_batch(&ids).unwrap();
        
        assert_eq!(results.len(), 5);
        assert!(results.iter().all(|r| r.is_some()));
        
        store.clear().unwrap();
        assert_eq!(store.count(), 0);
    }
}
