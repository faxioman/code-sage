
use super::{ToolHandlers, ensure_absolute_path, validate_codebase_path};
use crate::{Result};
use crate::ast::CodeChunker;
use crate::types::{IndexStats, CodeChunk};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::{info, warn, error};

#[derive(Debug, Deserialize)]
pub struct IndexCodebaseArgs {
    pub path: String,
    #[serde(default)]
    pub force: bool,
    #[serde(default = "default_splitter")]
    pub splitter: String,
    #[serde(default)]
    pub custom_extensions: Vec<String>,
    #[serde(default)]
    pub ignore_patterns: Vec<String>,
}

fn default_splitter() -> String {
    "ast".to_string()
}

impl ToolHandlers {
    pub async fn handle_index_codebase(&self, args: IndexCodebaseArgs) -> Result<String> {
        let IndexCodebaseArgs {
            path: codebase_path,
            force,
            splitter,
            custom_extensions,
            ignore_patterns,
        } = args;

        if splitter != "ast" && splitter != "langchain" {
            return Ok(serde_json::json!({
                "error": format!("Invalid splitter type '{}'. Must be 'ast' or 'langchain'.", splitter)
            }).to_string());
        }

        let absolute_path = ensure_absolute_path(&codebase_path)?;
        
        if let Err(e) = validate_codebase_path(&absolute_path) {
            return Ok(serde_json::json!({
                "error": format!("{}. Original input: '{}'", e, codebase_path)
            }).to_string());
        }

        let mut snapshot = self.snapshot_manager.lock().await;
        
        if snapshot.is_indexing(&absolute_path) {
            return Ok(serde_json::json!({
                "error": format!(
                    "Codebase '{}' is already being indexed in the background. Please wait for completion.",
                    absolute_path.display()
                )
            }).to_string());
        }

        let should_try_incremental = !force && snapshot.is_indexed(&absolute_path);
        
        if force {
            if snapshot.is_indexed(&absolute_path) {
                info!("[FORCE-REINDEX] Removing '{}' from indexed list for re-indexing", absolute_path.display());
                let _ = snapshot.remove_codebase(&absolute_path);
            }
            
            let mut vector_db = self.get_vector_db(&absolute_path)?;
            if vector_db.has_index(&absolute_path).await? {
                info!("[FORCE-REINDEX] Clearing index for '{}'", absolute_path.display());
                vector_db.delete_index(&absolute_path).await?;
            }
            
            let mut bm25 = self.get_bm25_search(&absolute_path)?;
            if bm25.has_index(&absolute_path).await? {
                bm25.delete_index(&absolute_path).await?;
            }
            
            use crate::sync::FileSynchronizer;
            let _ = FileSynchronizer::delete_snapshot(&absolute_path, &self.config.storage.data_dir).await;
        }

        snapshot.set_indexing(&absolute_path, 0)?;
        snapshot.save()?;
        
        drop(snapshot);

        let path_info = if codebase_path != absolute_path.to_string_lossy() {
            format!("\nNote: Input path '{}' was resolved to absolute path '{}'", codebase_path, absolute_path.display())
        } else {
            String::new()
        };

        let extension_info = if !custom_extensions.is_empty() {
            format!("\nUsing {} custom extensions: {}", custom_extensions.len(), custom_extensions.join(", "))
        } else {
            String::new()
        };

        let ignore_info = if !ignore_patterns.is_empty() {
            format!("\nUsing {} custom ignore patterns: {}", ignore_patterns.len(), ignore_patterns.join(", "))
        } else {
            String::new()
        };

        let handlers_clone = Arc::new(self.clone());
        let abs_path_clone = absolute_path.clone();
        let splitter_clone = splitter.clone();
        let custom_ext_clone = custom_extensions.clone();
        let ignore_pat_clone = ignore_patterns.clone();
        
        tokio::spawn(async move {
            if let Err(e) = handlers_clone.start_background_indexing(
                abs_path_clone,
                force,
                should_try_incremental,
                splitter_clone,
                custom_ext_clone,
                ignore_pat_clone,
            ).await {
                error!("[BACKGROUND-INDEX] Indexing failed: {}", e);
            }
        });

        Ok(serde_json::json!({
            "message": format!(
                "Started background indexing for codebase '{}' using {} splitter.{}{}{}\n\nIndexing is running in the background. You can search the codebase while indexing is in progress, but results may be incomplete until indexing completes.",
                absolute_path.display(),
                splitter.to_uppercase(),
                path_info,
                extension_info,
                ignore_info
            )
        }).to_string())
    }
}

impl ToolHandlers {
    async fn start_background_indexing(
        &self,
        absolute_path: PathBuf,
        _force_reindex: bool,
        should_try_incremental: bool,
        splitter_type: String,
        custom_extensions: Vec<String>,
        ignore_patterns: Vec<String>,
    ) -> Result<()> {
        info!("[BACKGROUND-INDEX] Starting background indexing for: {}", absolute_path.display());

        let mut last_save_time = std::time::Instant::now();

        if splitter_type != "ast" {
            warn!(
                "[BACKGROUND-INDEX] Non-AST splitter '{}' requested; falling back to AST splitter",
                splitter_type
            );
        }

        info!("[BACKGROUND-INDEX] Using embedding provider: {} with dimension: {}", 
            self.embedding.provider_name(),
            self.embedding.dimension()
        );

        if should_try_incremental {
            info!("[BACKGROUND-INDEX] Attempting incremental sync...");
            
            match self.try_incremental_sync(&absolute_path).await {
                Ok(Some(changes)) if changes.added.is_empty() && changes.removed.is_empty() && changes.modified.is_empty() => {
                    info!("[BACKGROUND-INDEX] No changes detected via incremental sync. Index is up to date.");
                    
                    let mut snapshot = self.snapshot_manager.lock().await;
                    let metadata_store = self.get_metadata_store(&absolute_path).await?;
                    let total_chunks = metadata_store.lock().await.count();
                    let files = self.scan_codebase(&absolute_path, &custom_extensions, &ignore_patterns).await?;
                    let indexed_files = files.len();
                    
                    let stats = IndexStats {
                        indexed_files,
                        total_chunks,
                        elapsed_secs: 0.0,
                        index_status: "completed".to_string(),
                    };
                    
                    snapshot.set_indexed(&absolute_path, stats)?;
                    snapshot.save()?;
                    return Ok(());
                }
                Ok(Some(changes)) => {
                    info!("[BACKGROUND-INDEX] Incremental sync detected {} changes ({} added, {} removed, {} modified)",
                        changes.added.len() + changes.removed.len() + changes.modified.len(),
                        changes.added.len(),
                        changes.removed.len(),
                        changes.modified.len()
                    );
                    
                    return self.process_incremental_changes(&absolute_path, changes).await;
                }
                Ok(None) => {
                    info!("[BACKGROUND-INDEX] No sync snapshot found. Falling back to full indexing.");
                }
                Err(e) => {
                    warn!("[BACKGROUND-INDEX] Incremental sync failed: {}. Falling back to full indexing.", e);
                }
            }
        }

        
        info!("[BACKGROUND-INDEX] Starting indexing with {} splitter for: {}", 
            splitter_type, 
            absolute_path.display()
        );

        if !custom_extensions.is_empty() {
            info!("[BACKGROUND-INDEX] Using custom extensions: {:?}", custom_extensions);
        }
        if !ignore_patterns.is_empty() {
            info!("[BACKGROUND-INDEX] Using custom ignore patterns: {:?}", ignore_patterns);
        }

        let files = self.scan_codebase(&absolute_path, &custom_extensions, &ignore_patterns).await?;
        let total_files = files.len();
        
        info!("[BACKGROUND-INDEX] Found {} files to process", total_files);
        let mut all_chunks = Vec::new();
        let chunker = CodeChunker::new(self.config.indexing.chunk_size, self.config.indexing.chunk_overlap);

        for (idx, file_path) in files.iter().enumerate() {
            let progress = ((idx as f32 / total_files as f32) * 30.0) as u8;
            if last_save_time.elapsed().as_secs() >= 2 {
                let mut snapshot = self.snapshot_manager.lock().await;
                snapshot.set_indexing(&absolute_path, progress)?;
                snapshot.save()?;
                last_save_time = std::time::Instant::now();
                info!("[BACKGROUND-INDEX] Progress: {:.1}% ({}/{})", progress, idx, total_files);
            }

            match self.process_file(file_path, &absolute_path, &chunker).await {
                Ok(mut chunks) => {
                    all_chunks.append(&mut chunks);
                }
                Err(e) => {
                    warn!("[BACKGROUND-INDEX] Failed to process file {}: {}", file_path.display(), e);
                    continue;
                }
            }

            if all_chunks.len() >= 450_000 {
                warn!("[BACKGROUND-INDEX] Chunk limit (450,000) reached. Stopping indexing.");
                break;
            }
        }

        let total_chunks = all_chunks.len();
        info!("[BACKGROUND-INDEX] Generated {} chunks from {} files", total_chunks, total_files);

        info!("[BACKGROUND-INDEX] Generating embeddings...");
        let embeddings = self.generate_embeddings_batch(&all_chunks, &absolute_path).await?;
        {
            let mut snapshot = self.snapshot_manager.lock().await;
            snapshot.set_indexing(&absolute_path, 60)?;
            snapshot.save()?;
        }

        info!("[BACKGROUND-INDEX] Storing vectors...");
        {
            let mut vector_db = self.get_vector_db(&absolute_path)?;
            vector_db.insert_batch(&absolute_path, &all_chunks, &embeddings).await?;
            info!("[BACKGROUND-INDEX] Saving vector index...");
            vector_db.save().await?;
            info!("[BACKGROUND-INDEX] Vector index saved successfully");
        }
        {
            let mut snapshot = self.snapshot_manager.lock().await;
            snapshot.set_indexing(&absolute_path, 85)?;
            snapshot.save()?;
        }

        info!("[BACKGROUND-INDEX] Building BM25 index...");
        {
            let mut bm25 = self.get_bm25_search(&absolute_path)?;
            bm25.insert_batch(&absolute_path, &all_chunks).await?;
        }
        {
            let mut snapshot = self.snapshot_manager.lock().await;
            snapshot.set_indexing(&absolute_path, 95)?;
            snapshot.save()?;
        }
        
        info!("[BACKGROUND-INDEX] Storing chunk metadata...");
        {
            let metadata_store = self.get_metadata_store(&absolute_path).await?;
            metadata_store.lock().await.insert_batch(&all_chunks)?;
            info!("[BACKGROUND-INDEX] Stored metadata for {} chunks", all_chunks.len());
        }
        let stats = IndexStats {
            indexed_files: total_files,
            total_chunks,
            elapsed_secs: 0.0, // TODO: track actual time
            index_status: if all_chunks.len() >= 450_000 {
                "limit_reached".to_string()
            } else {
                "completed".to_string()
            },
        };

        {
            let mut snapshot = self.snapshot_manager.lock().await;
            let _ = snapshot.set_indexed(&absolute_path, stats.clone());
            snapshot.save()?;
        }

        info!(
            "[BACKGROUND-INDEX] Indexing completed! Files: {}, Chunks: {}",
            stats.indexed_files,
            stats.total_chunks
        );

        Ok(())
    }
}

impl ToolHandlers {
    async fn scan_codebase(
        &self, 
        path: &PathBuf,
        custom_extensions: &[String],
        additional_ignore_patterns: &[String],
    ) -> Result<Vec<PathBuf>> {
        use ignore::WalkBuilder;
        
        let mut files = Vec::new();
        
        let mut extensions = self.config.indexing.supported_extensions.clone();
        for ext in custom_extensions {
            if !ext.starts_with('.') {
                extensions.push(format!(".{ext}"));
            } else {
                extensions.push(ext.clone());
            }
        }

        let mut builder = WalkBuilder::new(path);
        builder
            .follow_links(false)
            .git_ignore(true)          // Respect .gitignore
            .git_global(true)          // Respect global gitignore
            .git_exclude(true)         // Respect .git/info/exclude
            .ignore(true)              // Respect .ignore files
            .hidden(false);            // Don't index hidden files
        
        if !additional_ignore_patterns.is_empty() {
            use ignore::overrides::OverrideBuilder;
            let mut override_builder = OverrideBuilder::new(path);
            
            for pattern in additional_ignore_patterns {
                let _ = override_builder.add(&format!("!{pattern}"));
            }
            
            if let Ok(overrides) = override_builder.build() {
                builder.overrides(overrides);
                info!("[SCAN] Applied {} custom ignore patterns", additional_ignore_patterns.len());
            }
        }
        
        let walker = builder.build();

        for entry in walker {
            let entry = entry?;
            
            if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
                continue;
            }

            let file_path = entry.path();
            
            if let Some(ext) = file_path.extension() {
                let ext_str = format!(".{}", ext.to_string_lossy());
                if extensions.contains(&ext_str) {
                    files.push(file_path.to_path_buf());
                }
            }
        }

        info!("[SCAN] Found {} files after .gitignore filtering", files.len());
        if !custom_extensions.is_empty() {
            info!("[SCAN] Using {} extensions (including {} custom)", 
                extensions.len(), 
                custom_extensions.len()
            );
        }
        
        Ok(files)
    }

    async fn process_file(
        &self,
        file_path: &PathBuf,
        codebase_path: &PathBuf,
        chunker: &CodeChunker,
    ) -> Result<Vec<CodeChunk>> {
        let content = tokio::fs::read_to_string(file_path).await?;
        if content.len() > 1_000_000 {
            warn!("[PROCESS-FILE] Skipping large file (>1MB): {}", file_path.display());
            return Ok(Vec::new());
        }

        let language = self.detect_language(file_path)?;
        let relative_path = file_path.strip_prefix(codebase_path)
            .unwrap_or(file_path)
            .to_string_lossy()
            .to_string();

        let chunks = chunker.chunk_code(&content, &language, file_path, &relative_path)?;

        if chunks.len() > 50 {
            info!("[PROCESS-FILE] Large file: {} generated {} chunks", file_path.display(), chunks.len());
        }

        Ok(chunks)
    }

    fn detect_language(&self, path: &Path) -> Result<String> {
        use crate::types::Language;
        
        let ext = path.extension()
            .and_then(|e| e.to_str())
            .map(|s| format!(".{s}"))
            .unwrap_or_else(|| String::from("."));
        
        let language = Language::from_extension(&ext);
        Ok(language.as_str().to_string())
    }

    async fn generate_embeddings_batch(
        &self,
        chunks: &[CodeChunk],
        absolute_path: &Path,
    ) -> Result<Vec<Vec<f32>>> {
        // Use batch size of 16 (matching claude-context default behavior)
        let batch_size = 16;
        let mut all_embeddings = Vec::new();
        let total_batches = chunks.len().div_ceil(batch_size);
        let mut last_save_time = std::time::Instant::now();

        for (i, chunk_batch) in chunks.chunks(batch_size).enumerate() {
            let texts: Vec<String> = chunk_batch.iter().map(|c| c.content.clone()).collect();
            
            let batch_progress = (i as f32 / total_batches as f32) * 30.0;
            let progress = (30.0 + batch_progress) as u8;
            if last_save_time.elapsed().as_secs() >= 2 {
                let mut snapshot = self.snapshot_manager.lock().await;
                snapshot.set_indexing(absolute_path, progress)?;
                snapshot.save()?;
                last_save_time = std::time::Instant::now();
            }
            
            info!("[EMBEDDINGS] Processing batch {}/{} ({} chunks) - Progress: {}%", 
                i + 1, 
                total_batches,
                texts.len(),
                progress
            );

            let embeddings = self.embedding.embed_batch(&texts).await?;
            all_embeddings.extend(embeddings);
        }

        Ok(all_embeddings)
    }

    async fn try_incremental_sync(
        &self,
        codebase_path: &Path
    ) -> Result<Option<crate::sync::FileChanges>> {
        let sync_arc = self.get_or_create_synchronizer(codebase_path).await?;
        let mut sync = sync_arc.lock().await;
        let changes = sync.check_for_changes().await?;
        Ok(Some(changes))
    }

    async fn process_incremental_changes(
        &self,
        codebase_path: &Path,
        changes: crate::sync::FileChanges,
    ) -> Result<()> {
        let total_changes = changes.added.len() + changes.removed.len() + changes.modified.len();
        info!("[INCREMENTAL] Processing {} changes", total_changes);

        let metadata_store = self.get_metadata_store(codebase_path).await?;
        let mut vector_db = self.get_vector_db(codebase_path)?;
        let mut bm25 = self.get_bm25_search(codebase_path)?;

        for removed_file in &changes.removed {
            info!("[INCREMENTAL] Deleting chunks for removed file: {}", removed_file);
            let chunk_ids = self.find_chunk_ids_for_file(codebase_path, removed_file).await?;
            
            if !chunk_ids.is_empty() {
                {
                    let store = metadata_store.lock().await;
                    for chunk_id in &chunk_ids {
                        let _ = store.delete(chunk_id);
                    }
                }
                vector_db.delete(&chunk_ids).await?;
                bm25.delete(&chunk_ids)?;
                info!("[INCREMENTAL] Deleted {} chunks for {}", chunk_ids.len(), removed_file);
            }
        }

        // Delete chunks for modified files
        for modified_file in &changes.modified {
            info!("[INCREMENTAL] Deleting old chunks for modified file: {}", modified_file);
            let chunk_ids = self.find_chunk_ids_for_file(codebase_path, modified_file).await?;
            
            if !chunk_ids.is_empty() {
                {
                    let store = metadata_store.lock().await;
                    for chunk_id in &chunk_ids {
                        let _ = store.delete(chunk_id);
                    }
                }
                vector_db.delete(&chunk_ids).await?;
                bm25.delete(&chunk_ids)?;
                info!("[INCREMENTAL] Deleted {} old chunks for {}", chunk_ids.len(), modified_file);
            }
        }

        info!("[INCREMENTAL] Saving vector database after deletions...");
        vector_db.save().await?;
        info!("[INCREMENTAL] Vector database saved successfully");

        let files_to_index: Vec<_> = changes.added.iter()
            .chain(changes.modified.iter())
            .map(|rel_path| codebase_path.join(rel_path))
            .collect();

        if !files_to_index.is_empty() {
            info!("[INCREMENTAL] Re-indexing {} files", files_to_index.len());
            
            let chunker = CodeChunker::new(
                self.config.indexing.chunk_size,
                self.config.indexing.chunk_overlap,
            );
            
            let mut all_chunks = Vec::new();
            for file_path in files_to_index {
                match self.process_file(&file_path, &codebase_path.to_path_buf(), &chunker).await {
                    Ok(mut chunks) => {
                        all_chunks.append(&mut chunks);
                    }
                    Err(e) => {
                        warn!("[INCREMENTAL] Failed to process file {}: {}", file_path.display(), e);
                    }
                }
            }

            if !all_chunks.is_empty() {
                let embeddings = self.generate_embeddings_batch(&all_chunks, codebase_path).await?;
                let vector_docs: Vec<_> = all_chunks.iter()
                    .zip(embeddings.iter())
                    .map(|(chunk, embedding)| crate::vectordb::VectorDocument {
                        id: chunk.id.clone(),
                        vector: embedding.clone(),
                    })
                    .collect();
                vector_db.insert(vector_docs).await?;

                let bm25_docs: Vec<_> = all_chunks.iter()
                    .map(|chunk| crate::search::BM25Document {
                        id: chunk.id.clone(),
                        content: chunk.content.clone(),
                        file_path: chunk.file_path.to_string_lossy().to_string(),
                        start_line: chunk.start_line as u64,
                        end_line: chunk.end_line as u64,
                    })
                    .collect();
                bm25.insert(bm25_docs)?;

                metadata_store.lock().await.insert_batch(&all_chunks)?;

                info!("[INCREMENTAL] Successfully indexed {} chunks", all_chunks.len());
            }
        }

        let mut snapshot = self.snapshot_manager.lock().await;
        let total_chunks = metadata_store.lock().await.count();
        
        let files = self.scan_codebase(&codebase_path.to_path_buf(), &[], &[]).await?;
        let indexed_files = files.len();
        
        let stats = IndexStats {
            indexed_files,
            total_chunks,
            elapsed_secs: 0.0,
            index_status: "completed".to_string(),
        };
        
        snapshot.set_indexed(codebase_path, stats)?;
        snapshot.save()?;
        
        info!("[INCREMENTAL] Complete. Added: {}, Removed: {}, Modified: {}",
            changes.added.len(), changes.removed.len(), changes.modified.len());

        Ok(())
    }

    async fn find_chunk_ids_for_file(
        &self,
        codebase_path: &Path,
        relative_path: &str
    ) -> Result<Vec<String>> {
        let metadata_store = self.get_metadata_store(codebase_path).await?;
        let store = metadata_store.lock().await;
        
        let mut chunk_ids = Vec::new();
        for (chunk_id, metadata) in store.iter() {
            if metadata.relative_path == relative_path {
                chunk_ids.push(chunk_id);
            }
        }
        
        Ok(chunk_ids)
    }
}
