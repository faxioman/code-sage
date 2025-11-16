//! Search code handler
//! 
//! Handles the search_code MCP tool following claude-context logic

use super::{ToolHandlers, ensure_absolute_path, validate_codebase_path};
use crate::Result;
use crate::types::SearchResult;
use serde::Deserialize;
use std::path::Path;
use tracing::info;

#[derive(Debug, Deserialize)]
pub struct SearchCodeArgs {
    pub path: String,
    pub query: String,
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default)]
    pub extension_filter: Vec<String>,
}

fn default_limit() -> usize {
    10
}

impl ToolHandlers {
    /// Handle search_code tool call - returns JSON string
    pub async fn handle_search_code(&self, args: SearchCodeArgs) -> Result<String> {
        let SearchCodeArgs {
            path: codebase_path,
            query,
            limit,
            extension_filter,
        } = args;

        let result_limit = limit.min(50); // Cap at 50 like claude-context

        let absolute_path = ensure_absolute_path(&codebase_path)?;

        if let Err(e) = validate_codebase_path(&absolute_path) {
            return Ok(serde_json::json!({
                "error": format!("{}. Original input: '{}'", e, codebase_path)
            }).to_string());
        }

        let snapshot = self.snapshot_manager.lock().await;

        let is_indexed = snapshot.is_indexed(&absolute_path);
        let is_indexing = snapshot.is_indexing(&absolute_path);

        if !is_indexed && !is_indexing {
            return Ok(serde_json::json!({
                "error": format!(
                    "Codebase '{}' is not indexed. Please index it first using the index_codebase tool.",
                    absolute_path.display()
                )
            }).to_string());
        }

        let indexing_status_message = if is_indexing {
            "\n**Indexing in Progress**: This codebase is currently being indexed in the background. Search results may be incomplete until indexing completes."
        } else {
            ""
        };

        drop(snapshot);

        info!("[SEARCH] Searching in codebase: {}", absolute_path.display());
        info!("[SEARCH] Query: \"{}\"", query);
        info!("[SEARCH] Indexing status: {}", if is_indexing { "In Progress" } else { "Completed" });

        info!("[SEARCH] Using embedding provider: {} for search", self.embedding.provider_name());
        info!("[SEARCH] Generating embeddings for query using {}...", self.embedding.provider_name());

        if !extension_filter.is_empty() {
            for ext in &extension_filter {
                if !ext.starts_with('.') || ext.len() <= 1 || ext.contains(' ') {
                    return Ok(serde_json::json!({
                        "error": format!(
                            "Invalid file extension in extensionFilter: '{}'. Use proper extensions like '.ts', '.py'.",
                            ext
                        )
                    }).to_string());
                }
            }
        }

        let query_embedding = self.embedding.embed(&query).await?;

        let search_results = self.hybrid_search_with_filter(
            &absolute_path,
            &query,
            query_embedding.as_slice(),
            result_limit,
            &extension_filter,
        ).await?;

        info!("[SEARCH] Search completed! Found {} results using {} embeddings",
            search_results.len(),
            self.embedding.provider_name()
        );

        if search_results.is_empty() {
            let mut no_results_message = format!(
                "No results found for query: \"{}\" in codebase '{}'",
                query,
                absolute_path.display()
            );

            if is_indexing {
                no_results_message.push_str(
                    "\n\nNote: This codebase is still being indexed. Try searching again after indexing completes, or the query may not match any indexed content."
                );
            }

            return Ok(serde_json::json!({
                "message": no_results_message
            }).to_string());
        }

        let formatted_results = self.format_search_results(&search_results, &absolute_path);

        let mut result_message = format!(
            "Found {} results for query: \"{}\" in codebase '{}'{}",
            search_results.len(),
            query,
            absolute_path.display(),
            indexing_status_message
        );

        result_message.push_str("\n\n");
        result_message.push_str(&formatted_results);

        if is_indexing {
            result_message.push_str(
                "\n\n**Tip**: This codebase is still being indexed. More results may become available as indexing progresses."
            );
        }

        Ok(serde_json::json!({
            "message": result_message,
            "results_count": search_results.len()
        }).to_string())
    }
}

impl ToolHandlers {
    /// Perform hybrid search with optional extension filter
    async fn hybrid_search_with_filter(
        &self,
        codebase_path: &Path,
        query_text: &str,
        query_embedding: &[f32],
        limit: usize,
        extension_filter: &[String],
    ) -> Result<Vec<SearchResult>> {
        let vector_results = {
            let vector_db = self.get_vector_db(codebase_path)?;
            vector_db.search_codebase(codebase_path, query_embedding, 50).await?
        };

        let bm25_results = {
            let bm25 = self.get_bm25_search(codebase_path)?;
            bm25.search_codebase(codebase_path, query_text, 50).await?
        };

        let hybrid_search = self.get_hybrid_search();
        let combined_results = hybrid_search.rerank(vector_results, bm25_results);

        let metadata_store = self.get_metadata_store(codebase_path).await?;
        let metadata_store_guard = metadata_store.lock().await;
        
        let mut results = Vec::new();
        for (rank, (chunk_id, score)) in combined_results.iter().enumerate() {
            if let Some(metadata) = metadata_store_guard.get(chunk_id)? {
                let result = SearchResult {
                    file_path: metadata.file_path.clone(),
                    relative_path: metadata.relative_path.clone(),
                    start_line: metadata.start_line,
                    end_line: metadata.end_line,
                    content: metadata.content.clone(),
                    language: metadata.language.clone(),
                    score: *score,
                    rank: rank + 1,
                };
                results.push(result);
            }
        }
        
        if !extension_filter.is_empty() {
            results.retain(|result| {
                if let Some(ext) = std::path::Path::new(&result.file_path).extension() {
                    let ext_str = format!(".{}", ext.to_string_lossy());
                    extension_filter.contains(&ext_str)
                } else {
                    false
                }
            });
        }

        results.truncate(limit);
        Ok(results)
    }

    fn format_search_results(&self, results: &[SearchResult], codebase_path: &Path) -> String {
        let codebase_name = codebase_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");

        results
            .iter()
            .enumerate()
            .map(|(index, result)| {
                let location = format!(
                    "{}:{}-{}",
                    result.relative_path,
                    result.start_line,
                    result.end_line
                );

                let context = self.truncate_content(&result.content, 5000);

                format!(
                    "{}. Code snippet ({}) [{}]\n   Location: {}\n   Rank: {}\n   Context: \n```{}\n{}\n```\n",
                    index + 1,
                    result.language,
                    codebase_name,
                    location,
                    index + 1,
                    result.language,
                    context
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn truncate_content(&self, content: &str, max_length: usize) -> String {
        if content.len() <= max_length {
            content.to_string()
        } else {
            let truncated = &content[..max_length];
            format!("{truncated}...\n[Content truncated]")
        }
    }
}
