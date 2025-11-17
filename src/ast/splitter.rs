
use crate::{CodeChunk, ChunkMetadata, Language, Error, Result};
use sha2::{Sha256, Digest};
use std::path::Path;
use std::collections::HashSet;
use tree_sitter::{Parser, Node};

/// Context for chunk creation operations
struct ChunkContext<'a> {
    language: Language,
    file_path: &'a Path,
    relative_path: &'a str,
}

pub struct AstSplitter {
    chunk_size: usize,
    overlap: usize,
}

impl AstSplitter {
    pub fn new(chunk_size: usize, overlap: usize) -> Self {
        Self {
            chunk_size,
            overlap,
        }
    }

    pub fn chunk_code(
        &self,
        content: &str,
        language: &str,
        file_path: &Path,
        relative_path: &str,
    ) -> Result<Vec<CodeChunk>> {
        let lang = language.parse::<Language>().unwrap_or(Language::Unknown);
        
        // For unknown languages, go directly to fallback
        if lang == Language::Unknown {
            tracing::info!("Unknown language for {:?}, using character-based fallback", file_path);
            return self.split_with_fallback(content, lang, file_path, relative_path);
        }
        
        // Try AST-based splitting first
        match self.split_with_ast(content, lang, file_path, relative_path) {
            Ok(chunks) if !chunks.is_empty() => Ok(chunks),
            _ => {
                // Fallback to character-based splitting
                tracing::warn!("AST parsing failed for {:?}, using character-based fallback", file_path);
                self.split_with_fallback(content, lang, file_path, relative_path)
            }
        }
    }

    fn split_with_ast(
        &self,
        content: &str,
        language: Language,
        file_path: &Path,
        relative_path: &str,
    ) -> Result<Vec<CodeChunk>> {
        // Create parser for language
        let mut parser = Parser::new();
        
        // Get tree-sitter language
        let ts_lang: tree_sitter::Language = match language {
            Language::Rust => tree_sitter_rust::LANGUAGE.into(),
            Language::Python => tree_sitter_python::LANGUAGE.into(),
            Language::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
            Language::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Language::Java => tree_sitter_java::LANGUAGE.into(),
            Language::C => tree_sitter_c::LANGUAGE.into(),
            Language::Cpp => tree_sitter_cpp::LANGUAGE.into(),
            Language::Go => tree_sitter_go::LANGUAGE.into(),
            Language::CSharp => tree_sitter_c_sharp::LANGUAGE.into(),
            Language::Swift => tree_sitter_swift::LANGUAGE.into(),
            Language::Kotlin => tree_sitter_kotlin::language(),
            Language::Ruby => tree_sitter_ruby::LANGUAGE.into(),
            Language::Elixir => tree_sitter_elixir::LANGUAGE.into(),
            Language::ObjectiveC => tree_sitter_objc::LANGUAGE.into(),
            Language::Php => tree_sitter_php::LANGUAGE_PHP.into(),
            Language::Scala => tree_sitter_scala::LANGUAGE.into(),
            Language::Markdown => tree_sitter_md::LANGUAGE.into(),
            Language::Json => tree_sitter_json::LANGUAGE.into(),
            Language::Yaml => tree_sitter_yaml::LANGUAGE.into(),
            Language::Xml => tree_sitter_xml::LANGUAGE_XML.into(),
            Language::Html => tree_sitter_html::LANGUAGE.into(),
            Language::Css => tree_sitter_css::LANGUAGE.into(),
            Language::Scss => tree_sitter_scss::language(),
            Language::Toml => tree_sitter_toml_ng::LANGUAGE.into(),
            _ => return Err(Error::TreeSitter("Language not supported for AST parsing".to_string())),
        };
        
        // Set language on the parser
        parser.set_language(&ts_lang)
            .map_err(|e| Error::TreeSitter(format!("Failed to set language: {e}")))?;
        
        let tree = parser.parse(content, None)
            .ok_or_else(|| Error::TreeSitter("Failed to parse code".to_string()))?;
        
        let root_node = tree.root_node();
        
        let chunks = self.extract_chunks_from_ast(
            root_node,
            content,
            language,
            file_path,
            relative_path,
        )?;
        
        if chunks.is_empty() {
            return Err(Error::TreeSitter("No chunks extracted".to_string()));
        }
        
        Ok(chunks)
    }

    fn extract_chunks_from_ast(
        &self,
        node: Node,
        content: &str,
        language: Language,
        file_path: &Path,
        relative_path: &str,
    ) -> Result<Vec<CodeChunk>> {
        let mut raw_chunks = Vec::new();
        
        // Get splittable node types for this language
        let splittable_types = self.get_splittable_node_types(&language);
        
        // Traverse AST and extract semantic chunks
        Self::traverse_and_extract(node, content, &splittable_types, &mut raw_chunks);
        
        if raw_chunks.is_empty() {
            raw_chunks.push((
                content.to_string(),
                1,
                content.lines().count().max(1),
            ));
        }
        
        let ctx = ChunkContext {
            language,
            file_path,
            relative_path,
        };
        
        let mut chunks = Vec::new();
        for (chunk_index, (chunk_content, start_line, end_line)) in raw_chunks.into_iter().enumerate() {
            if chunk_content.len() > self.chunk_size {
                let refined = self.refine_large_chunk(
                    &chunk_content,
                    start_line,
                    end_line,
                    chunk_index,
                    &ctx,
                )?;
                chunks.extend(refined);
            } else {
                let chunk = self.create_code_chunk(
                    chunk_content,
                    start_line,
                    end_line,
                    chunk_index,
                    &ctx,
                )?;
                chunks.push(chunk);
            }
        }
        
        let chunks_with_overlap = self.add_overlap_to_chunks(chunks, content);
        let deduplicated = self.deduplicate_chunks(chunks_with_overlap);
        
        Ok(deduplicated)
    }
    
    fn deduplicate_chunks(&self, chunks: Vec<CodeChunk>) -> Vec<CodeChunk> {
        let mut seen_ids = HashSet::new();
        let original_count = chunks.len();
        
        let deduplicated: Vec<CodeChunk> = chunks
            .into_iter()
            .filter(|chunk| seen_ids.insert(chunk.id.clone()))
            .collect();
        
        let removed_count = original_count - deduplicated.len();
        if removed_count > 0 {
            tracing::debug!(
                "Deduplicated {} chunks with duplicate IDs (kept {}/{})",
                removed_count,
                deduplicated.len(),
                original_count
            );
        }
        
        deduplicated
    }

    fn get_splittable_node_types(&self, language: &Language) -> Vec<&'static str> {
        match language {
            Language::JavaScript => vec![
                "function_declaration",
                "arrow_function",
                "class_declaration",
                "method_definition",
                "export_statement",
            ],
            Language::TypeScript => vec![
                "function_declaration",
                "arrow_function",
                "class_declaration",
                "method_definition",
                "export_statement",
                "interface_declaration",
                "type_alias_declaration",
            ],
            Language::Python => vec![
                "function_definition",
                "class_definition",
                "decorated_definition",
                "async_function_definition",
            ],
            Language::Java => vec![
                "method_declaration",
                "class_declaration",
                "interface_declaration",
                "constructor_declaration",
            ],
            Language::Cpp | Language::C => vec![
                "function_definition",
                "class_specifier",
                "namespace_definition",
                "declaration",
            ],
            Language::Go => vec![
                "function_declaration",
                "method_declaration",
                "type_declaration",
                "var_declaration",
                "const_declaration",
            ],
            Language::Rust => vec![
                "function_item",
                "impl_item",
                "struct_item",
                "enum_item",
                "trait_item",
                "mod_item",
            ],
            Language::CSharp => vec![
                "method_declaration",
                "class_declaration",
                "interface_declaration",
                "struct_declaration",
                "enum_declaration",
            ],
            Language::Scala => vec![
                "method_declaration",
                "class_declaration",
                "interface_declaration",
                "constructor_declaration",
            ],
            Language::Swift => vec![
                "function_declaration",
                "class_declaration",
                "protocol_declaration",
                "struct_declaration",
                "enum_declaration",
            ],
            Language::Kotlin => vec![
                "function_declaration",
                "class_declaration",
                "object_declaration",
                "interface_declaration",
            ],
            Language::Ruby => vec![
                "method",
                "class",
                "module",
                "singleton_method",
            ],
            Language::Elixir => vec![
                "call",
                "anonymous_function",
                "do_block",
                "stab_clause",
            ],
            Language::ObjectiveC => vec![
                "function_definition",
                "class_interface",
                "class_implementation",
                "protocol_declaration",
            ],
            Language::Php => vec![
                "function_definition",
                "class_declaration",
                "method_declaration",
                "interface_declaration",
            ],
            Language::Markdown => vec![
                "section",
                "atx_heading",
                "setext_heading",
                "fenced_code_block",
                "indented_code_block",
            ],
            Language::Html => vec![
                "element",
                "script_element",
                "style_element",
            ],
            Language::Xml => vec![
                "element",
                "STag",
            ],
            Language::Css => vec![
                "rule_set",
                "media_statement",
                "keyframes_statement",
                "import_statement",
            ],
            Language::Scss => vec![
                "rule_set",
                "media_statement",
                "keyframes_statement",
                "import_statement",
                "mixin_statement",
                "function_statement",
                "include_statement",
            ],
            Language::Json => vec![
                "object",
                "array",
            ],
            Language::Yaml => vec![
                "block_mapping",
                "block_sequence",
                "flow_mapping",
                "flow_sequence",
            ],
            Language::Toml => vec![
                "table",
                "table_array_element",
            ],
            _ => vec![],
        }
    }

    fn traverse_and_extract(
        node: Node,
        content: &str,
        splittable_types: &[&str],
        chunks: &mut Vec<(String, usize, usize)>,
    ) {
        if splittable_types.contains(&node.kind()) {
            let start_line = node.start_position().row + 1;
            let end_line = node.end_position().row + 1;
            let start_byte = node.start_byte();
            let end_byte = node.end_byte();
            
            if let Some(node_text) = content.get(start_byte..end_byte) {
                if !node_text.trim().is_empty() {
                    chunks.push((
                        node_text.to_string(),
                        start_line,
                        end_line,
                    ));
                }
            }
        }
        
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            Self::traverse_and_extract(child, content, splittable_types, chunks);
        }
    }

    fn refine_large_chunk(
        &self,
        chunk_content: &str,
        start_line: usize,
        _end_line: usize,
        base_index: usize,
        ctx: &ChunkContext,
    ) -> Result<Vec<CodeChunk>> {
        let lines: Vec<&str> = chunk_content.lines().collect();
        let mut sub_chunks = Vec::new();
        let mut current_content = String::new();
        let mut current_start_line = start_line;
        let mut current_line_count = 0;
        let mut sub_index = 0;

        for (i, line) in lines.iter().enumerate() {
            let line_with_newline = if i == lines.len() - 1 {
                line.to_string()
            } else {
                format!("{line}\n")
            };

            if current_content.len() + line_with_newline.len() > self.chunk_size && !current_content.is_empty() {
                let chunk = self.create_code_chunk(
                    current_content.trim().to_string(),
                    current_start_line,
                    current_start_line + current_line_count - 1,
                    base_index * 1000 + sub_index,
                    ctx,
                )?;
                sub_chunks.push(chunk);

                current_content = line_with_newline;
                current_start_line = start_line + i;
                current_line_count = 1;
                sub_index += 1;
            } else {
                current_content.push_str(&line_with_newline);
                current_line_count += 1;
            }
        }

        if !current_content.trim().is_empty() {
            let chunk = self.create_code_chunk(
                current_content.trim().to_string(),
                current_start_line,
                current_start_line + current_line_count - 1,
                base_index * 1000 + sub_index,
                ctx,
            )?;
            sub_chunks.push(chunk);
        }

        Ok(sub_chunks)
    }

    fn create_code_chunk(
        &self,
        content: String,
        start_line: usize,
        end_line: usize,
        chunk_index: usize,
        ctx: &ChunkContext,
    ) -> Result<CodeChunk> {
        let mut hasher = Sha256::new();
        hasher.update(ctx.file_path.to_string_lossy().as_bytes());
        hasher.update(b":");
        hasher.update(start_line.to_string().as_bytes());
        hasher.update(b":");
        hasher.update(end_line.to_string().as_bytes());
        let id = format!("{:x}", hasher.finalize());
        
        let mut content_hasher = Sha256::new();
        content_hasher.update(&content);
        let content_hash = format!("{:x}", content_hasher.finalize());
        
        Ok(CodeChunk {
            id,
            content,
            file_path: ctx.file_path.to_path_buf(),
            relative_path: ctx.relative_path.to_string(),
            start_line,
            end_line,
            language: ctx.language.as_str().to_string(),
            metadata: ChunkMetadata {
                file_extension: ctx.file_path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_string(),
                chunk_index,
                hash: content_hash,
            },
        })
    }

    fn add_overlap_to_chunks(&self, chunks: Vec<CodeChunk>, _content: &str) -> Vec<CodeChunk> {
        if chunks.len() <= 1 || self.overlap == 0 {
            return chunks;
        }

        let mut overlapped = Vec::new();
        
        for (i, chunk) in chunks.iter().enumerate() {
            let mut new_content = chunk.content.clone();
            let mut new_start_line = chunk.start_line;

            if i > 0 && self.overlap > 0 {
                let prev_chunk = &chunks[i - 1];
                let overlap_len = self.overlap.min(prev_chunk.content.len());
                
                let target_start = if prev_chunk.content.len() > overlap_len {
                    prev_chunk.content.len() - overlap_len
                } else {
                    0
                };
                
                let mut safe_start = target_start;
                while safe_start < prev_chunk.content.len() && !prev_chunk.content.is_char_boundary(safe_start) {
                    safe_start += 1;
                }
                
                let overlap_text = &prev_chunk.content[safe_start..];
                
                let overlap_line_count = overlap_text.lines().count();
                new_content = format!("{overlap_text}\\n{new_content}");
                new_start_line = new_start_line.saturating_sub(overlap_line_count);
            }

            let mut hasher = Sha256::new();
            hasher.update(chunk.file_path.to_string_lossy().as_bytes());
            hasher.update(b":");
            hasher.update(chunk.start_line.to_string().as_bytes());
            hasher.update(b":");
            hasher.update(chunk.end_line.to_string().as_bytes());
            let new_id = format!("{:x}", hasher.finalize());

            overlapped.push(CodeChunk {
                id: new_id,
                content: new_content,
                file_path: chunk.file_path.clone(),
                relative_path: chunk.relative_path.clone(),
                start_line: new_start_line,
                end_line: chunk.end_line,
                language: chunk.language.clone(),
                metadata: chunk.metadata.clone(),
            });
        }

        overlapped
    }

    fn split_with_fallback(
        &self,
        content: &str,
        language: Language,
        file_path: &Path,
        relative_path: &str,
    ) -> Result<Vec<CodeChunk>> {
        let mut chunks = Vec::new();
        let mut chunk_index = 0;
        let content_len = content.len();

        let mut byte_pos = 0;
        while byte_pos < content_len {
            let target_end = (byte_pos + self.chunk_size).min(content_len);
            
            let mut end_pos = target_end;
            while end_pos > byte_pos && !content.is_char_boundary(end_pos) {
                end_pos -= 1;
            }
            
            if end_pos == byte_pos {
                end_pos = byte_pos + 1;
                while end_pos < content_len && !content.is_char_boundary(end_pos) {
                    end_pos += 1;
                }
            }
            
            let chunk_content = &content[byte_pos..end_pos];
            
            let text_before = &content[0..byte_pos];
            let start_line = text_before.lines().count() + 1;
            let chunk_lines = chunk_content.lines().count();
            let end_line = start_line + chunk_lines.max(1) - 1;
            
            let mut hasher = Sha256::new();
            hasher.update(file_path.to_string_lossy().as_bytes());
            hasher.update(b":");
            hasher.update(start_line.to_string().as_bytes());
            hasher.update(b":");
            hasher.update(end_line.to_string().as_bytes());
            let id = format!("{:x}", hasher.finalize());
            
            let mut content_hasher = Sha256::new();
            content_hasher.update(chunk_content.as_bytes());
            let content_hash = format!("{:x}", content_hasher.finalize());
            
            let chunk = CodeChunk {
                id,
                content: chunk_content.to_string(),
                file_path: file_path.to_path_buf(),
                relative_path: relative_path.to_string(),
                start_line,
                end_line,
                language: language.as_str().to_string(),
                metadata: ChunkMetadata {
                    file_extension: file_path
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("")
                        .to_string(),
                    chunk_index,
                    hash: content_hash,
                },
            };
            
            chunks.push(chunk);
            chunk_index += 1;
            
            if end_pos >= content_len {
                break;
            }
            
            let next_start = if self.overlap > 0 && end_pos > self.overlap {
                let target_start = end_pos - self.overlap;
                let mut safe_start = target_start;
                while safe_start < content_len && !content.is_char_boundary(safe_start) {
                    safe_start += 1;
                }
                safe_start.max(byte_pos + 1)
            } else {
                end_pos
            };
            
            byte_pos = next_start;
        }

        Ok(chunks)
    }
}

pub fn split_code(
    content: &str,
    language: Language,
    file_path: &Path,
    chunk_size: usize,
    overlap: usize,
) -> Result<Vec<CodeChunk>> {
    let splitter = AstSplitter::new(chunk_size, overlap);
    splitter.chunk_code(content, language.as_str(), file_path, "")
}
