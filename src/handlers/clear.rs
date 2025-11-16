
use super::{ToolHandlers, ensure_absolute_path, validate_codebase_path};
use crate::Result;
use serde::Deserialize;
use tracing::{info, error};

#[derive(Debug, Deserialize)]
pub struct ClearIndexArgs {
    pub path: String,
}

impl ToolHandlers {
    pub async fn handle_clear_index(&self, args: ClearIndexArgs) -> Result<String> {
        let ClearIndexArgs { path: codebase_path } = args;

        // Check if any codebases are indexed
        {
            let snapshot = self.snapshot_manager.lock().await;
            if snapshot.get_indexed_codebases().is_empty() && snapshot.get_indexing_codebases().is_empty() {
                return Ok(serde_json::json!({
                    "message": "No codebases are currently indexed or being indexed."
                }).to_string());
            }
        }

        let absolute_path = ensure_absolute_path(&codebase_path)?;
        if let Err(e) = validate_codebase_path(&absolute_path) {
            return Ok(serde_json::json!({
                "error": format!("{}. Original input: '{}'", e, codebase_path)
            }).to_string());
        }

        let (is_indexed, is_indexing) = {
            let snapshot = self.snapshot_manager.lock().await;
            (
                snapshot.is_indexed(&absolute_path),
                snapshot.is_indexing(&absolute_path),
            )
        };

        if !is_indexed && !is_indexing {
            return Ok(serde_json::json!({
                "error": format!(
                    "Codebase '{}' is not indexed or being indexed.",
                    absolute_path.display()
                )
            }).to_string());
        }

        info!("[CLEAR] Clearing codebase: {}", absolute_path.display());

        match self.get_vector_db(&absolute_path) {
            Ok(mut db) => {
                if let Err(e) = db.delete_index(&absolute_path).await {
                    let error_msg = format!("Failed to clear vector index for {}: {}", absolute_path.display(), e);
                    error!("[CLEAR] {}", error_msg);
                    return Ok(serde_json::json!({"error": error_msg}).to_string());
                }
                info!("[CLEAR] Successfully cleared vector index for: {}", absolute_path.display());
            }
            Err(e) => {
                let error_msg = format!("Failed to get vector database for {}: {}", absolute_path.display(), e);
                error!("[CLEAR] {}", error_msg);
                return Ok(serde_json::json!({"error": error_msg}).to_string());
            }
        }

        match self.get_bm25_search(&absolute_path) {
            Ok(mut search) => {
                if let Err(e) = search.delete_index(&absolute_path).await {
                    let error_msg = format!("Failed to clear BM25 index for {}: {}", absolute_path.display(), e);
                    error!("[CLEAR] {}", error_msg);
                    return Ok(serde_json::json!({"error": error_msg}).to_string());
                }
                info!("[CLEAR] Successfully cleared BM25 index for: {}", absolute_path.display());
            }
            Err(e) => {
                let error_msg = format!("Failed to get BM25 search for {}: {}", absolute_path.display(), e);
                error!("[CLEAR] {}", error_msg);
                return Ok(serde_json::json!({"error": error_msg}).to_string());
            }
        }
        
        match crate::metadata::MetadataStore::delete_for_codebase(&absolute_path, &self.config.storage.data_dir) {
            Ok(_) => {
                info!("[CLEAR] Successfully cleared metadata for: {}", absolute_path.display());
            }
            Err(e) => {
                tracing::warn!("[CLEAR] Failed to clear metadata (non-critical): {}", e);
            }
        }

        {
            let mut snapshot = self.snapshot_manager.lock().await;
            let _ = snapshot.remove_codebase(&absolute_path);
            snapshot.save()?;
        }

        info!("[CLEAR] Successfully cleared all indices for: {}", absolute_path.display());

        let (remaining_indexed, remaining_indexing) = {
            let snapshot = self.snapshot_manager.lock().await;
            (
                snapshot.get_indexed_codebases().len(),
                snapshot.get_indexing_codebases().len(),
            )
        };

        let mut result_text = format!("Successfully cleared codebase '{}'", absolute_path.display());

        if remaining_indexed > 0 || remaining_indexing > 0 {
            result_text.push_str(&format!(
                "\n{remaining_indexed} other indexed codebase(s) and {remaining_indexing} indexing codebase(s) remain"
            ));
        }

        Ok(serde_json::json!({
            "message": result_text,
            "remaining_indexed": remaining_indexed,
            "remaining_indexing": remaining_indexing
        }).to_string())
    }
}
