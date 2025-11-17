# Architecture Documentation

## Design Philosophy

### Core Principles

1. **Performance First** - Leverage Rust's speed advantage for efficient processing
2. **Embedded Storage** - Zero external dependencies with USearch + Tantivy + Sled
3. **Semantic Understanding** - AST-based chunking for intelligent code analysis
4. **Hybrid Search** - Combine keyword and semantic search for optimal results
5. **Simplicity** - Clean JSON responses, no complex type hierarchies
6. **Developer Experience** - Works out of the box with minimal configuration

## System Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                      MCP Client                              │
│              (Claude Desktop, Cursor, etc.)                  │
└────────────────────┬────────────────────────────────────────┘
                     │ stdio (MCP Protocol)
                     ↓
┌─────────────────────────────────────────────────────────────┐
│                   code-sage                                  │
│                    (Rust Binary)                             │
│                                                              │
│  ┌──────────────────────────────────────────────────────┐  │
│  │              MCP Handlers                             │  │
│  │  • analyze_code     • find_code                        │  │
│  │  • delete_index     • check_status                     │  │
│  │  Returns: JSON String (auto-converts to MCP Content)  │  │
│  └────────┬─────────────────────────────┬────────────────┘  │
│           │                             │                    │
│           ↓                             ↓                    │
│  ┌─────────────────┐         ┌──────────────────┐          │
│  │  AST Chunker    │         │  Hybrid Search   │          │
│  │  (tree-sitter)  │         │  (RRF)           │          │
│  └────────┬────────┘         └────────┬─────────┘          │
│           │                           │                     │
│           ↓                           ↓                     │
│  ┌─────────────────┐         ┌──────────────────┐          │
│  │  Embeddings     │         │  Storage         │          │
│  │  • OpenAI       │←────────┤  • USearch       │          │
│  │  • LM Studio    │         │  • Tantivy       │          │
│  │  • Ollama       │         │  • Sled          │          │
│  └─────────────────┘         │  • Sled          │          │
│                              └──────────────────┘          │
└─────────────────────────────────────────────────────────────┘
```

## Component Deep Dive

### 1. MCP Handlers (`src/handlers/`)

**Purpose**: Implement MCP protocol tools

**Simplified Design** (Nov 2025 update):
- All handlers return `String` (JSON serialized)
- No complex Response/ContentBlock types
- Auto-conversion to MCP Content via `IntoContents` trait
- Direct error handling with JSON error objects

**Tools**:
- `analyze_code` - Start background code analysis
- `find_code` - Hybrid search query
- `delete_index` - Delete index
- `check_status` - Check progress

**Handler Structure**:
```rust
pub async fn handle_analyze_code(&self, args: AnalyzeCodeArgs) -> Result<String> {
    // ... validation logic ...
    
    // Return JSON directly
    Ok(serde_json::json!({
        "message": "Started analysis...",
        "status": "analyzing"
    }).to_string())
}
```

**Benefits of Our Design**:
- Reduced maintenance overhead
- Comprehensive test coverage
- Flexible response formatting
- Direct JSON control for better debugging

### 2. AST Chunker (`src/ast/`)

**Purpose**: Split code into semantic units

**Features**:
- Tree-sitter parsing for 20+ languages
- Semantic nodes (functions, classes, methods)
- **Character-based fallback**: For AST parsing failures or unsupported languages
- Configurable chunk size & overlap (default: 2500 chars, 300 overlap)

**File Selection Strategy**:
1. **Extension filtering FIRST**: Only files with supported extensions (60+ defaults) are processed
2. **Custom extensions**: Use `custom_extensions` parameter to add project-specific file types
3. **Then chunking**: Selected files are chunked with AST or character-based approach

**Chunking Strategy** (for files that pass extension filter):
1. **AST-supported language**: Tree-sitter parses code into semantic chunks (functions, classes, etc.)
2. **AST parsing fails**: Falls back to character-based splitting (2500 chars with 300 overlap)
3. **Unknown language** (e.g., .ini, .txt): Uses character-based splitting directly

**Supported Languages** (AST parsing):
- Rust, Python, JavaScript, TypeScript
- Java, C/C++, Go, C#
- Swift, Kotlin, Ruby, Elixir, Objective-C
- PHP, Scala
- Markdown

**Fallback Behavior**: For any unsupported or unparseable file, the system gracefully falls back to character-based chunking (2500 chars with 300 char overlap), ensuring comprehensive codebase coverage.

### 3. Embeddings (`src/embeddings/`)

**Trait**:
```rust
#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    async fn embed(&self, text: &str) -> Result<Vec<f32>>;
    async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>>;
    fn dimension(&self) -> usize;
    fn provider_name(&self) -> &str;
}
```

**Providers**:
- **OpenAI**: text-embedding-3-small, text-embedding-3-large (Cloud)
- **LM Studio**: OpenAI-compatible local server (Recommended for local embeddings)
- **Ollama**: nomic-embed-text, local models (Note: Unstable on macOS M1)

### 4. Vector Database (`src/vectordb/`)

**Technology**: USearch (embedded HNSW)

**Why USearch?**
- Embedded, no server
- Fast approximate nearest neighbor
- File-based persistence
- SIMD optimizations

**Interface**:
```rust
#[async_trait]
pub trait VectorDatabase: Send + Sync {
    async fn insert(&mut self, documents: Vec<VectorDocument>) -> Result<()>;
    async fn search(&self, query: &[f32], top_k: usize) -> Result<Vec<SearchResult>>;
    async fn delete(&mut self, ids: &[String]) -> Result<()>;
    async fn has_index(&self, path: &Path) -> Result<bool>;
    async fn delete_index(&mut self, path: &Path) -> Result<()>;
}
```

### 5. Full-Text Search (`src/search/`)

**Technology**: Tantivy (Lucene in Rust)

**Schema**:
```rust
schema = {
    id: STRING | STORED,
    content: TEXT,  // Tokenized for BM25
    file_path: STRING | STORED,
    start_line: STORED,
    end_line: STORED,
}
```

### 6. Hybrid Search (`src/search/hybrid.rs`)

**Algorithm**: RRF (Reciprocal Rank Fusion)

```rust
fn rerank(vector_results, bm25_results) -> Vec<(DocId, Score)> {
    for (rank, result) in results.enumerate() {
        score += 1.0 / (RRF_K + rank + 1)  // k=100
    }
    sort_by_score_desc()
}
```

**Flow**:
1. Vector search → top 50
2. BM25 search → top 50
3. RRF combine
4. Return top K

### 7. Snapshot Manager (`src/snapshot/`)

**Purpose**: Track indexing state

**Format** (JSON, v2):
```json
{
  "formatVersion": "v2",
  "codebases": {
    "/path/to/codebase": {
      "status": "indexed",
      "indexedFiles": 1234,
      "totalChunks": 5678,
      "indexStatus": "completed",
      "lastUpdated": "2025-11-08T..."
    }
  }
}
```

**States**:
- `indexing` - In progress (+ percentage)
- `indexed` - Completed (+ stats)
- `indexfailed` - Failed (+ error)
- `not_found` - Unknown

**Progress Tracking System** (Updated 2025-11-10):

The analysis progress is divided into granular phases to provide accurate feedback:

| Phase | Progress Range | Update Frequency | Notes |
|-------|---------------|------------------|-------|
| **File Processing** | 0-30% | Every 2 seconds | Updates as files are scanned and chunked |
| **Embedding Generation** | 30-60% | Per batch (16 chunks) | Updates for each embedding batch processed |
| **Vector DB Storage** | 60-85% | Single update | Marks completion at 60% |
| **BM25 Indexing** | 85-95% | Single update | Marks completion at 85% |
| **Metadata Storage** | 95-100% | Single update | Marks completion at 95% |

**Benefits of Granular Progress**:
- No sudden jumps (e.g., 0% → 75%)
- Visible progress during embedding generation (typically the slowest phase)
- More accurate representation of actual work completed
- Better user experience during long indexing operations

**Technical Implementation**:
- Progress updates are saved to snapshot every 2 seconds during active processing
- Embedding batch size: 16 chunks (matches claude-context)
- Client can poll status using `check_status` tool
- No MCP protocol streaming (follows claude-context pattern)

### 8. Metadata Storage (`src/metadata.rs`)

**Technology**: Sled (embedded KV store)

**Data**:
```rust
Key: chunk_id (String)
Value: StoredMetadata {
    file_path: String,
    relative_path: String,
    start_line: usize,
    end_line: usize,
    content: String,  // Full chunk content
    language: String,
    hash: String,
}
```

**Storage Pattern**:
```
data/
  metadata/
    {sha256_hash}/  <- Per-codebase Sled DB
      chunk_1 -> StoredMetadata (bincode)
      chunk_2 -> StoredMetadata (bincode)
```

## Data Flow

### Indexing Flow

```
1. User calls analyze_code("/path/to/repo")
   ↓
2. Handler validates, returns JSON {"message": "Started..."}
   ↓
3. Background task starts (sets progress to 0%)
   ↓
4. Scanner walks directory (respects .gitignore)
   ↓
5. For each file (Progress: 0-30%):
   a. Read content
   b. AST parse (tree-sitter)
   c. Extract semantic chunks
   d. Update progress every 2 seconds
   ↓
6. Generate embeddings (Progress: 30-60%):
   a. Batch chunks (16 at a time)
   b. Call embedding API
   c. Update progress per batch
   ↓
7. Store vectors (Progress: 60-85%):
   • USearch: vectors
   • Save index to disk
   • Mark complete at 60%
   ↓
8. Store full-text (Progress: 85-95%):
   • Tantivy: BM25 index
   • Mark complete at 85%
   ↓
9. Store metadata (Progress: 95-100%):
   • Sled: metadata (including content)
   • Mark complete at 95%
   ↓
10. Complete: mark as "indexed" (100%)
```

### Search Flow

```
1. User calls find_code("/path/to/repo", "auth logic")
   ↓
2. Validate index exists
   ↓
3. Generate query embedding
   ↓
4. Parallel search:
   ├─→ USearch: vector similarity (top 50)
   └─→ Tantivy: BM25 keyword (top 50) [TODO: extract keywords]
   ↓
5. RRF rerank (k=100)
   ↓
6. Load metadata from Sled for top K (includes content!)
   ↓
7. Format results as JSON string
   ↓
8. Return: {"message": "...", "results_count": N}
```

## Configuration

### Environment Variables

```env
# Embedding provider (openai or ollama)
EMBEDDING_PROVIDER=openai

# OpenAI
OPENAI_API_KEY=sk-...
EMBEDDING_MODEL=text-embedding-3-small

# LM Studio (Recommended for local, uses OpenAI-compatible API)
# EMBEDDING_PROVIDER=openai
# OPENAI_API_KEY=lm-studio
# EMBEDDING_BASE_URL=http://localhost:1234/v1
# EMBEDDING_MODEL=nomic-embed-text

# Ollama (Note: Unstable on macOS M1)
# EMBEDDING_PROVIDER=ollama
# EMBEDDING_BASE_URL=http://localhost:11434
# EMBEDDING_MODEL=nomic-embed-text

# Storage
DATA_DIR=./data

# Search
DEFAULT_TOP_K=10
MIN_SCORE=0.3
RRF_K=100

# Indexing
CHUNK_SIZE=2500
CHUNK_OVERLAP=300
BATCH_SIZE=100
MAX_CHUNKS=450000
```

### File Patterns

**Supported Extensions** (60+ total, organized by category):

**Programming Languages (AST Parsing)**:
```
.rs .py .pyi .js .jsx .mjs .cjs .ts .tsx .java .c .h .cpp .hpp .cc .cxx
.go .cs .swift .kt .kts .rb .rake .gemspec .ex .exs .m .mm .php .scala .sc .sbt
```

**Config/Markup (AST Parsing)**:
```
.json .yaml .yml .xml .html .htm .css .scss .sass .toml
```

**iOS/macOS Specific**:
```
.xib .storyboard .plist .xcconfig
```

**Android/Java Build**:
```
.gradle .properties
```

**Build Systems**:
```
.cmake .make
```

**.NET**:
```
.csproj .sln .config .props .targets
```

**Shell Scripts**:
```
.sh .bash .zsh .fish
```

**Documentation**:
```
.md .markdown .txt .rst
```

**Notebooks**:
```
.ipynb
```

**Character-Based Fallback** (no AST parser):
```
.ini .less (and any extension in custom_extensions without tree-sitter support)
```

**Custom Extensions**:
Can be provided via `analyze_code` tool's `custom_extensions` parameter:
```json
{
  "path": "/path/to/code",
  "custom_extensions": [".proto", ".sql", ".graphql"]
}
```

**Smart Ignore System** (Automatic):
- Respects `.gitignore` files (project and global)
- Respects `.ignore` files
- Respects `.git/info/exclude`
- Skips hidden files automatically
- Uses `ignore` crate for robust gitignore-style matching

**Custom Ignore Patterns**:
Additional patterns can be provided via `analyze_code` tool:
```json
{
  "path": "/path/to/code",
  "ignore_patterns": ["*.test.ts", "tmp/*", "generated/*"]
}
```

**Default Behavior**:
- No configuration needed for standard projects
- Works out of the box with any git repository
- Automatically excludes `node_modules`, `.git`, `target`, etc. via .gitignore
- Custom patterns complement (don't replace) .gitignore

## Error Handling

**Pattern**: `Result<T, Error>` everywhere

```rust
pub enum Error {
    Io(std::io::Error),
    Json(serde_json::Error),
    Reqwest(reqwest::Error),
    TreeSitter(String),
    VectorDb(String),
    FullText(String),
    Embedding(String),
    NotIndexed(String),
    InvalidPath(String),
    Config(String),
}
```

**Handler Error Response**:
```rust
// On error, return JSON with error field
Ok(serde_json::json!({
    "error": format!("Analysis failed: {}", e)
}).to_string())
```

**Recovery Strategies**:
- AST parse fail → character-based fallback
- Embedding API error → retry 3x with backoff
- Storage write fail → rollback, clean up
- Index corruption → delete, force re-index

## Performance Optimizations

1. **Batch Processing** - Optimized 16-chunk batches for embedding generation
2. **Parallel Processing** - Rayon for CPU-intensive operations
3. **Async I/O** - Tokio for non-blocking network and disk operations
4. **Memory Streaming** - Process files without loading entire codebase into memory
5. **Smart Limits** - Configurable chunk limits to prevent memory overflow
6. **Efficient Storage** - Memory-mapped USearch, segmented Tantivy indices
7. **Direct Serialization** - JSON responses without complex type overhead

## MCP Protocol Integration

**Library**: rmcp 0.8.5

**Features Used**:
- `macros` - For #[tool] and #[tool_handler]
- `server` - For ServerHandler trait
- `transport-io` - For stdio transport

**Key Components**:
```rust
// Tool definition
#[tool(name = "find_code", description = "...")]
async fn find_code(&self, params: Parameters<Value>) -> String {
    // Returns String, auto-converts to Content via IntoContents
}

// Server handler
#[tool_handler(router = self.tool_router)]
impl ServerHandler for EmbeddingsContextServer {
    fn get_info(&self) -> ServerInfo { ... }
}

// Transport
let service = server.serve(stdio()).await?;
```

**Auto-Conversion Magic**:
- `String` implements `IntoContents` → `Vec<Content::text(string)>`
- `Vec<Content>` → `CallToolResult`
- No manual wrapping needed!

## Testing Strategy

**Unit Tests**: Comprehensive module isolation
**Integration Tests**: End-to-end workflow validation
**Benchmarks**: Performance profiling and optimization

**Current Status**: Production-ready with comprehensive test coverage

## Future Enhancements

- [ ] Storage compression with zstd for reduced disk usage
- [ ] Additional embedding providers (Cohere, Voyage AI)
- [ ] Web-based management interface for easier operations
- [ ] Advanced keyword extraction for improved BM25 precision
- [ ] Real-time progress streaming via MCP protocol
- [ ] Multi-codebase support for enterprise environments
- [ ] Export/import functionality for index portability

## Known Limitations

- **BM25 Keywords**: Uses full content for BM25 (works well, but could be optimized with keyword extraction)
- **File Size**: 1MB per file limit
- **Compression**: No storage compression yet
- **Progress**: No real-time MCP progress notifications yet (progress saved every 2 seconds)

## Technical Achievements

### Core Infrastructure
✅ **MCP Protocol Compliance**
- Proper stdout/stderr separation for JSON-RPC communication
- Tracing-based logging system for debugging
- Clean error handling with structured responses

### Vector System
✅ **Universal Embedding Support**
- Dynamic dimension detection for any embedding model
- Robust USearch integration with proper memory management
- Unique chunk identification system

### Search Engine
✅ **Hybrid Search Implementation**
- RRF (Reciprocal Rank Fusion) for optimal result ranking
- BM25 full-text search with Tantivy
- Semantic vector search with cosine similarity

### Code Analysis
✅ **AST-Based Processing**
- Tree-sitter integration for 20+ programming languages
- Semantic chunking with character-based fallback
- Intelligent file filtering with gitignore support

**Status**: Production-ready with comprehensive testing

---

**Last Updated**: 2025-11-16
**Status**: ✅ Production-ready with optimized tool naming and comprehensive documentation!
