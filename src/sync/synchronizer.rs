
use super::merkle::MerkleDAG;
use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::fs;
use tracing::{info, warn};

type FileHashesFuture<'a> = std::pin::Pin<Box<dyn std::future::Future<Output = Result<HashMap<String, String>>> + Send + 'a>>;

#[derive(Debug, Clone)]
pub struct FileChanges {
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub modified: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct SnapshotData {
    file_hashes: HashMap<String, String>,
    merkle_dag: MerkleDAG,
}

pub struct FileSynchronizer {
    file_hashes: HashMap<String, String>,
    merkle_dag: MerkleDAG,
    root_dir: PathBuf,
    snapshot_path: PathBuf,
    ignore_patterns: Vec<String>,
}

impl FileSynchronizer {
    pub fn new(root_dir: PathBuf, data_dir: PathBuf, ignore_patterns: Vec<String>) -> Self {
        let snapshot_path = Self::get_snapshot_path(&root_dir, &data_dir);
        
        Self {
            file_hashes: HashMap::new(),
            merkle_dag: MerkleDAG::new(),
            root_dir,
            snapshot_path,
            ignore_patterns,
        }
    }

    fn get_snapshot_path(codebase_path: &Path, data_dir: &Path) -> PathBuf {
        let merkle_dir = data_dir.join("merkle");
        
        let normalized_path = codebase_path.canonicalize()
            .unwrap_or_else(|_| codebase_path.to_path_buf());
        let path_str = normalized_path.to_string_lossy();
        let hash = format!("{:x}", md5::compute(path_str.as_bytes()));
        
        merkle_dir.join(format!("{hash}.json"))
    }

    async fn hash_file(file_path: &Path) -> Result<String> {
        let metadata = fs::metadata(file_path).await?;
        if !metadata.is_file() {
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("Attempted to hash a directory: {}", file_path.display())
            )));
        }
        
        let content = fs::read(file_path).await?;
        let mut hasher = Sha256::new();
        hasher.update(&content);
        Ok(format!("{:x}", hasher.finalize()))
    }

    fn generate_file_hashes<'a>(&'a self, dir: &'a Path) -> FileHashesFuture<'a> {
        Box::pin(async move {
            let mut file_hashes = HashMap::new();
            
            let mut entries = match fs::read_dir(dir).await {
                Ok(entries) => entries,
                Err(e) => {
                    warn!("[Synchronizer] Cannot read directory {}: {}", dir.display(), e);
                    return Ok(file_hashes);
                }
            };

            while let Some(entry) = entries.next_entry().await? {
                let full_path = entry.path();
                let relative_path = full_path.strip_prefix(&self.root_dir)
                    .unwrap_or(&full_path)
                    .to_string_lossy()
                    .to_string();

                // Check if this path should be ignored
                let metadata = match fs::metadata(&full_path).await {
                    Ok(m) => m,
                    Err(e) => {
                        warn!("[Synchronizer] Cannot stat {}: {}", full_path.display(), e);
                        continue;
                    }
                };

                if self.should_ignore(&relative_path, metadata.is_dir()) {
                    continue;
                }

                if metadata.is_dir() {
                    let sub_hashes = self.generate_file_hashes(&full_path).await?;
                    file_hashes.extend(sub_hashes);
                } else if metadata.is_file() {
                    match Self::hash_file(&full_path).await {
                        Ok(hash) => {
                            file_hashes.insert(relative_path, hash);
                        }
                        Err(e) => {
                            warn!("[Synchronizer] Cannot hash file {}: {}", full_path.display(), e);
                            continue;
                        }
                    }
                }
            }

            Ok(file_hashes)
        })
    }

    fn should_ignore(&self, relative_path: &str, is_directory: bool) -> bool {
        let path_parts: Vec<&str> = relative_path.split(std::path::MAIN_SEPARATOR).collect();
        if path_parts.iter().any(|part| part.starts_with('.')) {
            return true;
        }

        if self.ignore_patterns.is_empty() {
            return false;
        }

        let normalized_path = relative_path.replace('\\', "/");
        let normalized_path = normalized_path.trim_matches('/');

        if normalized_path.is_empty() {
            return false;
        }

        for pattern in &self.ignore_patterns {
            if self.match_pattern(normalized_path, pattern, is_directory) {
                return true;
            }
        }

        let normalized_path_parts: Vec<&str> = normalized_path.split('/').collect();
        for i in 0..normalized_path_parts.len() {
            let partial_path = normalized_path_parts[..=i].join("/");
            for pattern in &self.ignore_patterns {
                if pattern.ends_with('/') {
                    let dir_pattern = &pattern[..pattern.len() - 1];
                    if self.simple_glob_match(&partial_path, dir_pattern)
                        || self.simple_glob_match(normalized_path_parts[i], dir_pattern)
                    {
                        return true;
                    }
                }
                else if pattern.contains('/') {
                    if self.simple_glob_match(&partial_path, pattern) {
                        return true;
                    }
                }
                else if self.simple_glob_match(normalized_path_parts[i], pattern) {
                    return true;
                }
            }
        }

        false
    }

    fn match_pattern(&self, file_path: &str, pattern: &str, is_directory: bool) -> bool {
        let clean_path = file_path.trim_matches('/');
        let clean_pattern = pattern.trim_matches('/');

        if clean_path.is_empty() || clean_pattern.is_empty() {
            return false;
        }

        if pattern.ends_with('/') {
            if !is_directory {
                return false;
            }
            let dir_pattern = &clean_pattern[..clean_pattern.len() - 1];

            return self.simple_glob_match(clean_path, dir_pattern)
                || clean_path.split('/').any(|part| self.simple_glob_match(part, dir_pattern));
        }

        if clean_pattern.contains('/') {
            return self.simple_glob_match(clean_path, clean_pattern);
        }

        let file_name = clean_path.split('/').next_back().unwrap_or(clean_path);
        self.simple_glob_match(file_name, clean_pattern)
    }

    fn simple_glob_match(&self, text: &str, pattern: &str) -> bool {
        if text.is_empty() || pattern.is_empty() {
            return false;
        }

        let regex_pattern = regex::escape(pattern).replace("\\*", ".*");
        let regex = match regex::Regex::new(&format!("^{regex_pattern}$")) {
            Ok(r) => r,
            Err(_) => return false,
        };

        regex.is_match(text)
    }

    fn build_merkle_dag(file_hashes: &HashMap<String, String>) -> MerkleDAG {
        let mut dag = MerkleDAG::new();

        let mut values_string = String::new();
        for hash in file_hashes.values() {
            values_string.push_str(hash);
        }
        let root_node_data = format!("root:{values_string}");
        let root_node_id = dag.add_node(root_node_data, None);

        let mut sorted_paths: Vec<_> = file_hashes.keys().collect();
        sorted_paths.sort();

        for path in sorted_paths {
            let hash = file_hashes.get(path).unwrap();
            let file_data = format!("{path}:{hash}");
            dag.add_node(file_data, Some(root_node_id.clone()));
        }

        dag
    }

    pub async fn initialize(&mut self) -> Result<()> {
        info!("[Synchronizer] Initializing for {}", self.root_dir.display());
        self.load_snapshot().await?;
        self.merkle_dag = Self::build_merkle_dag(&self.file_hashes);
        info!("[Synchronizer] Initialized with {} file hashes", self.file_hashes.len());
        Ok(())
    }

    pub async fn check_for_changes(&mut self) -> Result<FileChanges> {
        info!("[Synchronizer] Checking for file changes...");

        // Generate new file hashes
        let new_file_hashes = self.generate_file_hashes(&self.root_dir).await?;
        let new_merkle_dag = Self::build_merkle_dag(&new_file_hashes);

        let dag_changes = MerkleDAG::compare(&self.merkle_dag, &new_merkle_dag);

        if !dag_changes.added.is_empty()
            || !dag_changes.removed.is_empty()
            || !dag_changes.modified.is_empty()
        {
            let file_changes = self.compare_states(&self.file_hashes, &new_file_hashes);

            self.file_hashes = new_file_hashes;
            self.merkle_dag = new_merkle_dag;
            self.save_snapshot().await?;

            info!(
                "[Synchronizer] Found changes: {} added, {} removed, {} modified",
                file_changes.added.len(),
                file_changes.removed.len(),
                file_changes.modified.len()
            );
            return Ok(file_changes);
        }

        info!("[Synchronizer] No changes detected based on Merkle DAG comparison");
        Ok(FileChanges {
            added: Vec::new(),
            removed: Vec::new(),
            modified: Vec::new(),
        })
    }

    fn compare_states(
        &self,
        old_hashes: &HashMap<String, String>,
        new_hashes: &HashMap<String, String>,
    ) -> FileChanges {
        let mut added = Vec::new();
        let mut removed = Vec::new();
        let mut modified = Vec::new();

        for (file, hash) in new_hashes {
            if !old_hashes.contains_key(file) {
                added.push(file.clone());
            } else if old_hashes.get(file) != Some(hash) {
                modified.push(file.clone());
            }
        }

        for file in old_hashes.keys() {
            if !new_hashes.contains_key(file) {
                removed.push(file.clone());
            }
        }

        FileChanges {
            added,
            removed,
            modified,
        }
    }

    pub fn get_file_hash(&self, file_path: &str) -> Option<&String> {
        self.file_hashes.get(file_path)
    }

    async fn save_snapshot(&self) -> Result<()> {
        if let Some(parent) = self.snapshot_path.parent() {
            fs::create_dir_all(parent).await?;
        }

        let snapshot = SnapshotData {
            file_hashes: self.file_hashes.clone(),
            merkle_dag: self.merkle_dag.clone(),
        };

        let json = serde_json::to_string_pretty(&snapshot)?;
        fs::write(&self.snapshot_path, json).await?;
        info!("[Synchronizer] Saved snapshot to {}", self.snapshot_path.display());
        Ok(())
    }

    async fn load_snapshot(&mut self) -> Result<()> {
        match fs::read_to_string(&self.snapshot_path).await {
            Ok(content) => {
                let snapshot: SnapshotData = serde_json::from_str(&content)?;
                self.file_hashes = snapshot.file_hashes;
                self.merkle_dag = snapshot.merkle_dag;
                info!("[Synchronizer] Loaded snapshot from {}", self.snapshot_path.display());
                Ok(())
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                self.file_hashes = self.generate_file_hashes(&self.root_dir).await?;
                self.merkle_dag = Self::build_merkle_dag(&self.file_hashes);
                self.save_snapshot().await?;
                Ok(())
            }
            Err(e) => Err(Error::Io(e)),
        }
    }

    pub async fn delete_snapshot(codebase_path: &Path, data_dir: &Path) -> Result<()> {
        let snapshot_path = Self::get_snapshot_path(codebase_path, data_dir);

        match fs::remove_file(&snapshot_path).await {
            Ok(_) => {
                info!("[Synchronizer] Deleted snapshot file: {}", snapshot_path.display());
                Ok(())
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                info!(
                    "[Synchronizer] Snapshot file not found (already deleted): {}",
                    snapshot_path.display()
                );
                Ok(())
            }
            Err(e) => {
                warn!(
                    "[Synchronizer] Failed to delete snapshot file {}: {}",
                    snapshot_path.display(),
                    e
                );
                Err(Error::Io(e))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_glob_match() {
        let data_dir = PathBuf::from("/tmp/data");
        let sync = FileSynchronizer::new(PathBuf::from("/tmp"), data_dir, vec![]);
        
        assert!(sync.simple_glob_match("test.js", "*.js"));
        assert!(sync.simple_glob_match("test.min.js", "*.min.js"));
        assert!(!sync.simple_glob_match("test.js", "*.ts"));
        assert!(sync.simple_glob_match("node_modules", "node_modules"));
    }

    #[test]
    fn test_should_ignore() {
        let data_dir = PathBuf::from("/tmp/data");
        let sync = FileSynchronizer::new(
            PathBuf::from("/tmp"),
            data_dir,
            vec![
                "node_modules".to_string(),
                ".git".to_string(),
                "*.log".to_string(),
            ],
        );

        assert!(sync.should_ignore("node_modules", true));
        assert!(sync.should_ignore("src/node_modules", true));
        assert!(sync.should_ignore(".git", true));
        assert!(sync.should_ignore("test.log", false));
        assert!(!sync.should_ignore("src/index.js", false));
    }
}
