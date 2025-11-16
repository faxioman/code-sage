use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("HTTP request error: {0}")]
    Reqwest(#[from] reqwest::Error),

    #[error("Tree-sitter parsing error: {0}")]
    TreeSitter(String),

    #[error("Vector database error: {0}")]
    VectorDb(String),

    #[error("Full-text search error: {0}")]
    FullText(String),

    #[error("Embedding error: {0}")]
    Embedding(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Codebase not indexed: {0}")]
    NotIndexed(String),

    #[error("Invalid path: {0}")]
    InvalidPath(String),

    #[error("Unsupported language: {0}")]
    UnsupportedLanguage(String),

    #[error("Snapshot error: {0}")]
    Snapshot(String),

    #[error("MCP protocol error: {0}")]
    Mcp(String),

    #[error("Task join error: {0}")]
    Join(#[from] tokio::task::JoinError),
    
    #[error("WalkDir error: {0}")]
    WalkDir(#[from] walkdir::Error),
    
    #[error("Ignore pattern error: {0}")]
    Ignore(#[from] ignore::Error),

    #[error("Unknown error: {0}")]
    Unknown(String),
}

pub type Result<T> = std::result::Result<T, Error>;
