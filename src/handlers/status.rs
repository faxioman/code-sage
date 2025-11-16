//! Get indexing status handler
//! 
//! Handles the get_indexing_status MCP tool following claude-context logic

use super::{ToolHandlers, ensure_absolute_path, validate_codebase_path};
use crate::Result;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct GetIndexingStatusArgs {
    pub path: String,
}

impl ToolHandlers {
    /// Handle get_indexing_status tool call - returns JSON string
    pub async fn handle_get_indexing_status(&self, args: GetIndexingStatusArgs) -> Result<String> {
        let GetIndexingStatusArgs { path: codebase_path } = args;

        // Force absolute path resolution
        let absolute_path = ensure_absolute_path(&codebase_path)?;

        // Validate path exists
        if let Err(e) = validate_codebase_path(&absolute_path) {
            return Ok(serde_json::json!({
                "error": format!("{}. Original input: '{}'", e, codebase_path)
            }).to_string());
        }

        let snapshot = self.snapshot_manager.lock().await;
        let status = snapshot.get_status(&absolute_path);

        let status_message = match status {
            crate::snapshot::CodebaseStatus::Indexed(info) => {
                let mut msg = format!(
                    "Codebase '{}' is fully indexed and ready for search.",
                    absolute_path.display()
                );
                msg.push_str(&format!(
                    "\nStatistics: {} files, {} chunks",
                    info.indexed_files,
                    info.total_chunks
                ));
                msg.push_str(&format!(
                    "\nStatus: {}",
                    info.index_status
                ));
                msg.push_str(&format!(
                    "\nLast updated: {}",
                    chrono::DateTime::from_timestamp(info.last_updated as i64, 0)
                        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                        .unwrap_or_else(|| "unknown".to_string())
                ));
                msg
            }

            crate::snapshot::CodebaseStatus::Indexing(info) => {
                let progress_percentage = info.indexing_percentage;
                let mut msg = format!(
                    "Codebase '{}' is currently being indexed. Progress: {:.1}%",
                    absolute_path.display(),
                    progress_percentage
                );

                if progress_percentage < 10.0 {
                    msg.push_str(" (Preparing and scanning files...)");
                } else if progress_percentage < 100.0 {
                    msg.push_str(" (Processing files and generating embeddings...)");
                }

                msg.push_str(&format!(
                    "\nLast updated: {}",
                    chrono::DateTime::from_timestamp(info.last_updated as i64, 0)
                        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                        .unwrap_or_else(|| "unknown".to_string())
                ));
                msg
            }

            crate::snapshot::CodebaseStatus::IndexFailed(info) => {
                let mut msg = format!(
                    "Codebase '{}' indexing failed.",
                    absolute_path.display()
                );
                msg.push_str(&format!(
                    "\nError: {}",
                    info.error_message
                ));
                if info.last_attempted_percentage > 0.0 {
                    msg.push_str(&format!(
                        "\nFailed at: {:.1}% progress",
                        info.last_attempted_percentage
                    ));
                }
                msg.push_str(&format!(
                    "\nFailed at: {}",
                    chrono::DateTime::from_timestamp(info.last_updated as i64, 0)
                        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                        .unwrap_or_else(|| "unknown".to_string())
                ));
                msg.push_str("\nYou can retry indexing by running the index_codebase command again.");
                msg
            }

            crate::snapshot::CodebaseStatus::NotFound => {
                format!(
                    "Codebase '{}' is not indexed. Please use the index_codebase tool to index it first.",
                    absolute_path.display()
                )
            }
        };

        let path_info = if codebase_path != absolute_path.to_string_lossy() {
            format!(
                "\nNote: Input path '{}' was resolved to absolute path '{}'",
                codebase_path,
                absolute_path.display()
            )
        } else {
            String::new()
        };

        Ok(serde_json::json!({
            "message": status_message + &path_info
        }).to_string())
    }
}
