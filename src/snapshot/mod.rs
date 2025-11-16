
use crate::{Result, IndexingStatus, IndexStats};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::collections::HashMap;
use chrono::Utc;

/// Codebase snapshot (v2 format)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "formatVersion")]
pub enum CodebaseSnapshot {
    #[serde(rename = "v2")]
    V2 {
        codebases: HashMap<String, CodebaseInfo>,
        #[serde(rename = "lastUpdated")]
        last_updated: String,
    },
}

/// Information about a codebase
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status")]
pub enum CodebaseInfo {
    #[serde(rename = "indexed")]
    Indexed {
        #[serde(rename = "indexedFiles")]
        indexed_files: usize,
        #[serde(rename = "totalChunks")]
        total_chunks: usize,
        #[serde(rename = "indexStatus")]
        index_status: String,
        #[serde(rename = "lastUpdated")]
        last_updated: String,
    },
    #[serde(rename = "indexing")]
    Indexing {
        #[serde(rename = "indexingPercentage")]
        indexing_percentage: u8,
        #[serde(rename = "lastUpdated")]
        last_updated: String,
    },
    #[serde(rename = "indexfailed")]
    IndexFailed {
        #[serde(rename = "errorMessage")]
        error_message: String,
        #[serde(rename = "lastAttemptedPercentage", skip_serializing_if = "Option::is_none")]
        last_attempted_percentage: Option<u8>,
        #[serde(rename = "lastUpdated")]
        last_updated: String,
    },
}

/// Status enum for handlers
pub enum CodebaseStatus {
    Indexed(IndexedStatusInfo),
    Indexing(IndexingStatusInfo),
    IndexFailed(FailedStatusInfo),
    NotFound,
}

#[derive(Debug, Clone)]
pub struct IndexedStatusInfo {
    pub indexed_files: usize,
    pub total_chunks: usize,
    pub index_status: String,
    pub last_updated: u64,
}

#[derive(Debug, Clone)]
pub struct IndexingStatusInfo {
    pub indexing_percentage: f32,
    pub last_updated: u64,
}

#[derive(Debug, Clone)]
pub struct FailedStatusInfo {
    pub error_message: String,
    pub last_attempted_percentage: f32,
    pub last_updated: u64,
}

pub struct SnapshotManager {
    snapshot_path: PathBuf,
    codebases: HashMap<String, CodebaseInfo>,
}

impl SnapshotManager {
    pub fn new(snapshot_path: PathBuf) -> Result<Self> {
        let mut manager = Self {
            snapshot_path,
            codebases: HashMap::new(),
        };
        
        if manager.snapshot_path.exists() {
            manager.load()?;
        }
        
        Ok(manager)
    }
    
    pub fn load(&mut self) -> Result<()> {
        if !self.snapshot_path.exists() {
            return Ok(());
        }
        
        let content = std::fs::read_to_string(&self.snapshot_path)?;
        let snapshot: CodebaseSnapshot = serde_json::from_str(&content)?;
        
        match snapshot {
            CodebaseSnapshot::V2 { codebases, .. } => {
                for (path, info) in codebases {
                    if Path::new(&path).exists() {
                        self.codebases.insert(path, info);
                    }
                }
            }
        }
        
        Ok(())
    }
    
    pub fn save(&self) -> Result<()> {
        if let Some(parent) = self.snapshot_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        
        let snapshot = CodebaseSnapshot::V2 {
            codebases: self.codebases.clone(),
            last_updated: Utc::now().to_rfc3339(),
        };
        
        let json = serde_json::to_string_pretty(&snapshot)?;
        std::fs::write(&self.snapshot_path, json)?;
        
        Ok(())
    }
    
    pub fn set_indexing(&mut self, path: &Path, progress: u8) -> Result<()> {
        let key = path.to_string_lossy().to_string();
        let info = CodebaseInfo::Indexing {
            indexing_percentage: progress,
            last_updated: Utc::now().to_rfc3339(),
        };
        self.codebases.insert(key, info);
        Ok(())
    }
    
    pub fn set_indexed(&mut self, path: &Path, stats: IndexStats) -> Result<()> {
        let key = path.to_string_lossy().to_string();
        let info = CodebaseInfo::Indexed {
            indexed_files: stats.indexed_files,
            total_chunks: stats.total_chunks,
            index_status: stats.index_status,
            last_updated: Utc::now().to_rfc3339(),
        };
        self.codebases.insert(key, info);
        Ok(())
    }
    
    pub fn set_failed(&mut self, path: &Path, error: String, last_progress: Option<u8>) -> Result<()> {
        let key = path.to_string_lossy().to_string();
        let info = CodebaseInfo::IndexFailed {
            error_message: error,
            last_attempted_percentage: last_progress,
            last_updated: Utc::now().to_rfc3339(),
        };
        self.codebases.insert(key, info);
        Ok(())
    }
    
    pub fn remove(&mut self, path: &Path) -> Result<()> {
        let key = path.to_string_lossy().to_string();
        self.codebases.remove(&key);
        Ok(())
    }
    
    pub fn is_indexing(&self, path: &Path) -> bool {
        let key = path.to_string_lossy().to_string();
        matches!(self.codebases.get(&key), Some(CodebaseInfo::Indexing { .. }))
    }
    
    pub fn is_indexed(&self, path: &Path) -> bool {
        let key = path.to_string_lossy().to_string();
        matches!(self.codebases.get(&key), Some(CodebaseInfo::Indexed { .. }))
    }
    
    pub fn remove_codebase(&mut self, path: &Path) -> Result<()> {
        self.remove(path)
    }
    
    pub fn get_indexed_codebases(&self) -> Vec<PathBuf> {
        self.codebases
            .iter()
            .filter_map(|(path, info)| {
                if matches!(info, CodebaseInfo::Indexed { .. }) {
                    Some(PathBuf::from(path))
                } else {
                    None
                }
            })
            .collect()
    }
    
    pub fn get_indexing_codebases(&self) -> Vec<PathBuf> {
        self.codebases
            .iter()
            .filter_map(|(path, info)| {
                if matches!(info, CodebaseInfo::Indexing { .. }) {
                    Some(PathBuf::from(path))
                } else {
                    None
                }
            })
            .collect()
    }
    
    pub fn get_indexing_progress(&self, path: &Path) -> u8 {
        let key = path.to_string_lossy().to_string();
        if let Some(CodebaseInfo::Indexing { indexing_percentage, .. }) = self.codebases.get(&key) {
            *indexing_percentage
        } else {
            0
        }
    }
    
    pub fn get_status(&self, path: &Path) -> CodebaseStatus {
        let key = path.to_string_lossy().to_string();
        
        match self.codebases.get(&key) {
            Some(CodebaseInfo::Indexed {
                indexed_files,
                total_chunks,
                index_status,
                last_updated,
            }) => {
                CodebaseStatus::Indexed(IndexedStatusInfo {
                    indexed_files: *indexed_files,
                    total_chunks: *total_chunks,
                    index_status: index_status.clone(),
                    last_updated: parse_timestamp(last_updated),
                })
            }
            Some(CodebaseInfo::Indexing {
                indexing_percentage,
                last_updated,
            }) => {
                CodebaseStatus::Indexing(IndexingStatusInfo {
                    indexing_percentage: *indexing_percentage as f32,
                    last_updated: parse_timestamp(last_updated),
                })
            }
            Some(CodebaseInfo::IndexFailed {
                error_message,
                last_attempted_percentage,
                last_updated,
            }) => {
                CodebaseStatus::IndexFailed(FailedStatusInfo {
                    error_message: error_message.clone(),
                    last_attempted_percentage: last_attempted_percentage.unwrap_or(0) as f32,
                    last_updated: parse_timestamp(last_updated),
                })
            }
            None => CodebaseStatus::NotFound,
        }
    }
    
    pub fn get_simple_status(&self, path: &Path) -> IndexingStatus {
        let key = path.to_string_lossy().to_string();
        
        match self.codebases.get(&key) {
            Some(CodebaseInfo::Indexed { .. }) => IndexingStatus::Indexed,
            Some(CodebaseInfo::Indexing { indexing_percentage, .. }) => {
                IndexingStatus::Indexing { progress: *indexing_percentage }
            }
            Some(CodebaseInfo::IndexFailed { error_message, .. }) => {
                IndexingStatus::Failed { error: error_message.clone() }
            }
            None => IndexingStatus::NotIndexed,
        }
    }
}

fn parse_timestamp(timestamp_str: &str) -> u64 {
    chrono::DateTime::parse_from_rfc3339(timestamp_str)
        .map(|dt| dt.timestamp() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    
    #[test]
    fn test_snapshot_v2_format() {
        let dir = tempdir().unwrap();
        let snapshot_path = dir.path().join("snapshot.json");
        let test_path = dir.path().join("test_codebase");
        std::fs::create_dir_all(&test_path).unwrap();
        
        let mut manager = SnapshotManager::new(snapshot_path.clone()).unwrap();
        
        // Set indexing
        manager.set_indexing(&test_path, 50).unwrap();
        manager.save().unwrap();
        assert_eq!(manager.get_simple_status(&test_path), IndexingStatus::Indexing { progress: 50 });
        
        // Set indexed
        let stats = IndexStats {
            indexed_files: 100,
            total_chunks: 500,
            elapsed_secs: 10.5,
            index_status: "completed".to_string(),
        };
        manager.set_indexed(&test_path, stats).unwrap();
        manager.save().unwrap();
        assert_eq!(manager.get_simple_status(&test_path), IndexingStatus::Indexed);
        
        // Reload from file
        let manager2 = SnapshotManager::new(snapshot_path).unwrap();
        assert_eq!(manager2.get_simple_status(&test_path), IndexingStatus::Indexed);
        
        let json = std::fs::read_to_string(&manager2.snapshot_path).unwrap();
        assert!(json.contains("\"formatVersion\"") && json.contains("\"v2\""));
        assert!(json.contains("\"indexedFiles\"") && json.contains("100"));
        assert!(json.contains("\"totalChunks\"") && json.contains("500"));
    }
}
