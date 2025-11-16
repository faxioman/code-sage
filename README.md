# Code Sage

A high-performance **MCP (Model Context Protocol) server** for semantic code search, written in Rust.

## Features

- **Hybrid Search**: Combines BM25 (keyword-based) + Vector embeddings (semantic) with RRF reranking
- **AST-Based Chunking**: Uses tree-sitter to intelligently split code into semantic units (functions, classes, methods)
  - **Character-based fallback**: For files where AST parsing fails or isn't available
  - **Comprehensive language support**: 60+ file extensions supported out of the box
- **Smart File Filtering**: 
  - Automatic .gitignore support (respects .gitignore, .ignore, .git/info/exclude)
  - Custom file extensions support for project-specific file types
  - No configuration needed - works out of the box
- **Embedded Storage**: Zero external dependencies - all data stored locally
  - USearch for vector similarity search
  - Tantivy for BM25 full-text search
  - Sled for metadata storage
- **Multiple Embedding Providers**: 
  - OpenAI (text-embedding-3-small, text-embedding-3-large)
  - **LM Studio (Recommended)** - OpenAI-compatible local embeddings with better stability
  - Ollama (local embeddings) - Note: Unstable on macOS M1 with some models
- **MCP Compatible**: Works with Claude Desktop, Cursor, and other MCP clients
- **Multi-Language Support**: 
  - **Programming Languages (AST)**: Rust, Python, JavaScript/TypeScript, Java, C/C++, Go, C#, Swift, Kotlin, Ruby, Objective-C, PHP, Scala
  - **Config/Markup (AST)**: JSON, YAML, XML, HTML, CSS, SCSS, TOML, Markdown
  - **iOS/macOS**: .xib, .storyboard, .plist (via XML parser), .xcconfig (via TOML parser)
  - **Android/Java**: .xml (layouts, manifests), .gradle, .properties
  - **Build Systems**: .cmake, .sbt, .make, Makefile, CMakeLists.txt
  - **Shell Scripts**: .sh, .bash, .zsh, .fish
  - **Character-based fallback**: .ini, .txt, .rst, and any extension added via `custom_extensions`

## Architecture

See [ARCHITECTURE.md](./ARCHITECTURE.md) for detailed architecture documentation.

**Key Design Decisions:**

1. **Hybrid Search over Pure Semantic**: Combines keyword and semantic search for better results
2. **Embedded over Client-Server**: Everything runs locally, no vector DB server needed
3. **AST-First with Fallback**: Semantic chunking when possible, character-based when needed
4. **Rust for Performance**: Efficient memory usage and fast processing

## Installation

### Prerequisites

- Rust 1.70+ (edition 2021)
- OpenAI API key (or Ollama running locally)

### Build from source

```bash
git clone https://github.com/faxioman/code-sage-mcp.git
cd code-sage
cargo build --release
```

The binary will be in `target/release/code-sage`

## Usage

### MCP Server Configuration

Add to your MCP client configuration (e.g., Claude Desktop):

#### LM Studio (Recommended)

```json
{
  "mcpServers": {
    "code-sage": {
      "command": "/path/to/code-sage",
      "env": {
        "EMBEDDING_PROVIDER": "openai",
        "OPENAI_API_KEY": "lm-studio",
        "EMBEDDING_BASE_URL": "http://localhost:1234/v1",
        "EMBEDDING_MODEL": "nomic-embed-text",
        "DATA_DIR": "./data"
      }
    }
  }
}
```

#### OpenAI (Cloud)

```json
{
  "mcpServers": {
    "code-sage": {
      "command": "/path/to/code-sage",
      "env": {
        "EMBEDDING_PROVIDER": "openai",
        "OPENAI_API_KEY": "sk-your-key-here",
        "EMBEDDING_MODEL": "text-embedding-3-small",
        "DATA_DIR": "./data"
      }
    }
  }
}
```

#### Ollama (Experimental)

```json
{
  "mcpServers": {
    "code-sage": {
      "command": "/path/to/code-sage",
      "env": {
        "EMBEDDING_PROVIDER": "ollama",
        "EMBEDDING_BASE_URL": "http://localhost:11434",
        "EMBEDDING_MODEL": "nomic-embed-text",
        "DATA_DIR": "./data"
      }
    }
  }
}
```

### Provider Setup

#### LM Studio (Recommended)

1. Download [LM Studio](https://lmstudio.ai/)
2. Search and download `nomic-embed-text` model
3. Go to "Local Server" tab
4. Click "Start Server" (default port: 1234)
5. Use the configuration above

#### Ollama

1. Install [Ollama](https://ollama.ai/)
2. Run: `ollama pull nomic-embed-text`
3. Start Ollama service
4. Use the configuration above

### Advanced Configuration

Optional parameters can be added to the `env` section:

```json
{
  "mcpServers": {
    "code-sage": {
      "command": "/path/to/code-sage",
      "env": {
        "EMBEDDING_PROVIDER": "openai",
        "OPENAI_API_KEY": "sk-your-key-here",
        "EMBEDDING_MODEL": "text-embedding-3-small",
        "DATA_DIR": "./data",
        "DEFAULT_TOP_K": "10",
        "MIN_SCORE": "0.3",
        "RRF_K": "100",
        "CHUNK_SIZE": "2500",
        "CHUNK_OVERLAP": "300",
        "BATCH_SIZE": "100",
        "MAX_CHUNKS": "450000"
      }
    }
  }
}
```

### Available MCP Tools

#### 1. `analyze_code`

Create a searchable index of your code by analyzing functions, classes, and methods:

```json
{
  "path": "/absolute/path/to/codebase",
  "force": false,
  "splitter": "ast",
  "custom_extensions": [".proto", ".sql"],
  "ignore_patterns": ["*.test.ts", "tmp/*"]
}
```

**Parameters**:
- `path` (required): Absolute path to codebase directory
- `force` (optional): Force re-analysis if already analyzed (default: false)
- `splitter` (optional): Chunking strategy - "ast" or "langchain" (default: "ast")
- `custom_extensions` (optional): Additional file extensions to analyze beyond the 60+ defaults (e.g., [".proto", ".graphql"])
- `ignore_patterns` (optional): Additional patterns to ignore (complements .gitignore)

**How File Selection Works**:
1. **Extension Filtering**: Only files with supported extensions are analyzed (60+ defaults)
2. **Gitignore Respecting**: Automatically respects `.gitignore`, `.ignore`, and `.git/info/exclude`
3. **Custom Extensions**: Use `custom_extensions` to add project-specific file types not in defaults
4. **Hidden Files**: Skipped by default

**Supported Extensions by Default** (60+ total):
- **Core Languages**: .rs, .py, .js, .jsx, .ts, .tsx, .java, .c, .h, .cpp, .hpp, .go, .cs, .swift, .kt, .rb, .m, .mm, .php, .scala
- **JS/TS Variants**: .mjs, .cjs
- **Config Formats**: .json, .yaml, .yml, .toml, .xml, .ini
- **iOS/macOS**: .xib, .storyboard, .plist, .xcconfig
- **Android/Java**: .gradle, .properties
- **Build Systems**: .cmake, .sbt, .make
- **Web/Styling**: .html, .htm, .css, .scss, .sass, .less
- **Shell Scripts**: .sh, .bash, .zsh, .fish
- **.NET**: .csproj, .sln, .config, .props, .targets
- **Ruby**: .gemspec, .rake
- **Docs**: .md, .markdown, .txt, .rst
- **Notebooks**: .ipynb

**Example - Adding Custom Extensions**:
```json
{
  "path": "/path/to/project",
  "custom_extensions": [".proto", ".graphql", ".vue", ".svelte"]
}
```

**Returns**: JSON with success/error message

#### 2. `find_code`

Find code using natural language questions:

```json
{
  "path": "/absolute/path/to/codebase",
  "query": "authentication logic",
  "limit": 10,
  "extension_filter": [".ts", ".js"]
}
```

**Returns**: JSON with search results and formatted code snippets

#### 3. `delete_index`

Delete the search index for a codebase:

```json
{
  "path": "/absolute/path/to/codebase"
}
```

**Returns**: JSON with confirmation message

#### 4. `check_status`

Check if code analysis is complete, in progress, or failed:

```json
{
  "path": "/absolute/path/to/codebase"
}
```

**Returns**: JSON with status (analyzed, analyzing with %, failed, or not found)

**Progress Tracking** (Updated 2025-11-10):
The analysis progress is divided into granular phases for accurate feedback:
- **0-30%**: File processing (scanning and chunking)
- **30-60%**: Embedding generation (updated per batch)
- **60-85%**: Vector database storage
- **85-95%**: BM25 full-text indexing
- **95-100%**: Metadata storage

This ensures smooth progress updates with no sudden jumps, providing better visibility into the analysis process.


## How It Works

### 1. Indexing Pipeline

```
Code Files
    ↓
AST Parsing (tree-sitter)
    ↓
Semantic Chunks (functions, classes)
    ↓
Embeddings (OpenAI/Ollama)
    ↓
Storage (USearch + Tantivy + Sled)
```

### 2. Hybrid Search

```
Query
    ↓
    ├─→ Vector Search (USearch) → Top 50 results
    │
    └─→ BM25 Search (Tantivy) → Top 50 results
    
    ↓
RRF Reranking (merge with k=100)
    ↓
Final Results (Top K)
```

### Development Setup

```bash
# Install dependencies
cargo build

# Run tests
cargo test

# Run with logging
RUST_LOG=debug cargo run

# Format code
cargo fmt

# Lint
cargo clippy
```

## Inspiration & Credits

This project is inspired by:
- [claude-context](https://github.com/zilliztech/claude-context) - Original TypeScript implementation
- Design decisions around hybrid search and AST chunking
- MCP protocol implementation patterns

**Key Differences:**
- Written in Rust for performance
- Embedded storage (no Milvus/Qdrant server needed)
- Simplified architecture
- Native binary (easier deployment)
- Simpler handler responses (JSON strings)

## License

MIT License - see [LICENSE](./LICENSE)

## Known Issues & Limitations

- **File Size Limit**: 1MB per file
- **Extension Filtering**: Files must have a supported extension or be added via `custom_extensions` to be analyzed
- **Storage**: No compression yet (working on it)
- **Switching Providers**: When changing embedding providers with different dimensions (e.g., from OpenAI 1536 to LM Studio 768), **delete the `data/` folder** before re-indexing to avoid dimension mismatch errors


## Support

- Issues: [GitHub Issues](https://github.com/faxioman/code-sage-mcp/issues)
- Discussions: [GitHub Discussions](https://github.com/faxioman/code-sage-mcp/discussions)

---

**Built with ❤️ in Rust** 🦀
