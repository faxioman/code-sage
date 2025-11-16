use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// A code chunk with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeChunk {
    pub id: String,
    pub content: String,
    pub file_path: PathBuf,
    pub relative_path: String,
    pub start_line: usize,
    pub end_line: usize,
    pub language: String,
    pub metadata: ChunkMetadata,
}

/// Metadata for a code chunk
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkMetadata {
    pub file_extension: String,
    pub chunk_index: usize,
    pub hash: String,
}

/// Search result from hybrid search
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub file_path: PathBuf,
    pub relative_path: String,
    pub start_line: usize,
    pub end_line: usize,
    pub content: String,
    pub language: String,
    pub score: f32,
    pub rank: usize,
}

/// Indexing statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexStats {
    pub indexed_files: usize,
    pub total_chunks: usize,
    pub elapsed_secs: f64,
    pub index_status: String,
}

/// Codebase indexing status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum IndexingStatus {
    NotIndexed,
    Indexing { progress: u8 },
    Indexed,
    Failed { error: String },
}

/// Language enum for supported programming languages
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum Language {
    Rust,
    Python,
    JavaScript,
    TypeScript,
    Java,
    C,
    Cpp,
    Go,
    CSharp,
    Swift,
    Kotlin,
    Ruby,
    ObjectiveC,
    Php,
    Scala,
    Markdown,
    Json,
    Yaml,
    Xml,
    Html,
    Css,
    Scss,
    Toml,
    Unknown,
}

impl Language {
    pub fn supported_extensions() -> Vec<String> {
        vec![
            ".rs", ".py", ".pyi", ".js", ".jsx", ".mjs", ".cjs",
            ".ts", ".tsx", ".java", ".c", ".h", ".cpp", ".hpp",
            ".cc", ".cxx", ".go", ".cs", ".swift", ".kt", ".kts",
            ".rb", ".rake", ".gemspec", ".m", ".mm", ".php",
            ".scala", ".sc", ".sbt",
            ".json", ".yaml", ".yml", ".toml", ".xml", ".xib",
            ".storyboard", ".plist", ".csproj", ".sln", ".props",
            ".targets", ".html", ".htm", ".css", ".scss", ".sass",
            ".less", ".xcconfig",
            ".md", ".markdown", ".rst",
            ".txt", ".sh", ".bash", ".zsh", ".fish",
            ".gradle", ".properties", ".config", ".cmake",
            ".make", ".ini", ".ipynb",
        ]
        .into_iter()
        .map(String::from)
        .collect()
    }

    pub fn from_extension(ext: &str) -> Self {
        match ext {
            ".rs" => Language::Rust,
            ".py" | ".pyi" => Language::Python,
            ".js" | ".jsx" | ".mjs" | ".cjs" => Language::JavaScript,
            ".ts" | ".tsx" => Language::TypeScript,
            ".java" => Language::Java,
            ".c" | ".h" => Language::C,
            ".cpp" | ".hpp" | ".cc" | ".cxx" => Language::Cpp,
            ".go" => Language::Go,
            ".cs" => Language::CSharp,
            ".swift" => Language::Swift,
            ".kt" | ".kts" => Language::Kotlin,
            ".rb" | ".rake" | ".gemspec" => Language::Ruby,
            ".m" | ".mm" => Language::ObjectiveC,
            ".php" => Language::Php,
            ".scala" | ".sc" | ".sbt" => Language::Scala,
            ".md" | ".markdown" | ".rst" => Language::Markdown,
            ".json" => Language::Json,
            ".yaml" | ".yml" => Language::Yaml,
            ".xml" | ".xib" | ".storyboard" | ".plist" | ".csproj" | ".sln" | ".props" | ".targets" => Language::Xml,
            ".html" | ".htm" => Language::Html,
            ".css" => Language::Css,
            ".scss" | ".sass" => Language::Scss,
            ".less" => Language::Css,
            ".toml" | ".xcconfig" => Language::Toml,
            _ => Language::Unknown,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Language::Rust => "rust",
            Language::Python => "python",
            Language::JavaScript => "javascript",
            Language::TypeScript => "typescript",
            Language::Java => "java",
            Language::C => "c",
            Language::Cpp => "cpp",
            Language::Go => "go",
            Language::CSharp => "csharp",
            Language::Swift => "swift",
            Language::Kotlin => "kotlin",
            Language::Ruby => "ruby",
            Language::ObjectiveC => "objc",
            Language::Php => "php",
            Language::Scala => "scala",
            Language::Markdown => "markdown",
            Language::Json => "json",
            Language::Yaml => "yaml",
            Language::Xml => "xml",
            Language::Html => "html",
            Language::Css => "css",
            Language::Scss => "scss",
            Language::Toml => "toml",
            Language::Unknown => "unknown",
        }
    }
}

impl std::str::FromStr for Language {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "rust" => Ok(Language::Rust),
            "python" => Ok(Language::Python),
            "javascript" | "js" => Ok(Language::JavaScript),
            "typescript" | "ts" => Ok(Language::TypeScript),
            "java" => Ok(Language::Java),
            "c" => Ok(Language::C),
            "cpp" | "c++" => Ok(Language::Cpp),
            "go" => Ok(Language::Go),
            "csharp" | "c#" => Ok(Language::CSharp),
            "swift" => Ok(Language::Swift),
            "kotlin" => Ok(Language::Kotlin),
            "ruby" => Ok(Language::Ruby),
            "objc" | "objective-c" | "objectivec" => Ok(Language::ObjectiveC),
            "php" => Ok(Language::Php),
            "scala" => Ok(Language::Scala),
            "markdown" | "md" => Ok(Language::Markdown),
            "json" => Ok(Language::Json),
            "yaml" | "yml" => Ok(Language::Yaml),
            "xml" => Ok(Language::Xml),
            "html" => Ok(Language::Html),
            "css" => Ok(Language::Css),
            "scss" | "sass" => Ok(Language::Scss),
            "toml" => Ok(Language::Toml),
            "unknown" => Ok(Language::Unknown),
            _ => Ok(Language::Unknown),
        }
    }
}
