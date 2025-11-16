
pub mod index;
pub mod search;
pub mod status;
pub mod clear;

pub use index::IndexCodebaseArgs;
pub use search::SearchCodeArgs;
pub use status::GetIndexingStatusArgs;
pub use clear::ClearIndexArgs;

use crate::{Result, Error, Config};
use crate::snapshot::SnapshotManager;
use crate::embeddings::EmbeddingProvider;
use crate::vectordb::{USearchDatabase, VectorDatabase};
use crate::search::{BM25Search, HybridSearch};
use crate::sync::FileSynchronizer;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::collections::HashMap;
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct ToolHandlers {
    config: Config,
    snapshot_manager: Arc<Mutex<SnapshotManager>>,
    embedding: Arc<dyn EmbeddingProvider>,
    synchronizers: Arc<Mutex<HashMap<String, Arc<Mutex<FileSynchronizer>>>>>,
    metadata_stores: Arc<Mutex<HashMap<String, Arc<Mutex<crate::metadata::MetadataStore>>>>>,
}

impl ToolHandlers {
    pub fn new(
        config: Config,
        snapshot_manager: SnapshotManager,
        embedding: Arc<dyn EmbeddingProvider>,
    ) -> Self {
        Self {
            config,
            snapshot_manager: Arc::new(Mutex::new(snapshot_manager)),
            embedding,
            synchronizers: Arc::new(Mutex::new(HashMap::new())),
            metadata_stores: Arc::new(Mutex::new(HashMap::new())),
        }
    }
    
    fn get_vector_db(&self, codebase_path: &Path) -> Result<Box<dyn VectorDatabase>> {
        let dimension = self.embedding.dimension();
        tracing::info!("[HANDLER] Creating/loading vector DB with dimension: {}", dimension);
        let db = USearchDatabase::for_codebase(codebase_path, dimension, &self.config.storage.data_dir)?;
        Ok(Box::new(db))
    }
    
    fn get_bm25_search(&self, codebase_path: &Path) -> Result<BM25Search> {
        BM25Search::for_codebase(codebase_path, &self.config.storage.data_dir)
    }
    
    fn get_hybrid_search(&self) -> HybridSearch {
        HybridSearch::new(self.config.search.rrf_k)
    }
    
    async fn get_metadata_store(&self, codebase_path: &Path) -> Result<Arc<Mutex<crate::metadata::MetadataStore>>> {
        let path_key = codebase_path.to_string_lossy().to_string();
        let mut stores = self.metadata_stores.lock().await;
        
        if let Some(store) = stores.get(&path_key) {
            Ok(Arc::clone(store))
        } else {
            let store = crate::metadata::MetadataStore::for_codebase(codebase_path, &self.config.storage.data_dir)?;
            let store_arc = Arc::new(Mutex::new(store));
            stores.insert(path_key, Arc::clone(&store_arc));
            Ok(store_arc)
        }
    }

    pub async fn get_or_create_synchronizer(
        &self,
        codebase_path: &Path
    ) -> Result<Arc<Mutex<FileSynchronizer>>> {
        let path_key = codebase_path.to_string_lossy().to_string();
        let mut syncs = self.synchronizers.lock().await;
        
        if let Some(sync) = syncs.get(&path_key) {
            Ok(Arc::clone(sync))
        } else {
            let mut sync = FileSynchronizer::new(
                codebase_path.to_path_buf(),
                self.config.storage.data_dir.clone(),
                self.config.indexing.ignore_patterns.clone(),
            );
            sync.initialize().await?;
            let sync_arc = Arc::new(Mutex::new(sync));
            syncs.insert(path_key, Arc::clone(&sync_arc));
            Ok(sync_arc)
        }
    }
}

pub fn ensure_absolute_path(path: &str) -> Result<PathBuf> {
    let path_buf = PathBuf::from(path);
    
    if path_buf.is_absolute() {
        Ok(path_buf)
    } else {
        let current_dir = std::env::current_dir()?;
        let absolute = current_dir.join(path_buf);
        
        tracing::warn!(
            "Relative path provided: '{}', resolved to absolute: '{}'",
            path,
            absolute.display()
        );
        
        Ok(absolute)
    }
}

pub fn validate_codebase_path(path: &Path) -> Result<()> {
    if !path.exists() {
        return Err(Error::InvalidPath(format!(
            "Path does not exist: {}",
            path.display()
        )));
    }
    
    if !path.is_dir() {
        return Err(Error::InvalidPath(format!(
            "Path is not a directory: {}",
            path.display()
        )));
    }
    
    Ok(())
}
