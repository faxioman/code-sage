use crate::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Embedding provider configuration
    pub embedding: EmbeddingConfig,
    
    /// Storage paths
    pub storage: StorageConfig,
    
    /// Search configuration
    pub search: SearchConfig,
    
    /// Indexing configuration
    pub indexing: IndexingConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingConfig {
    pub provider: EmbeddingProvider,
    pub api_key: Option<String>,
    pub model: String,
    pub base_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EmbeddingProvider {
    OpenAI,
    Ollama,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    pub data_dir: PathBuf,
    pub vectors_dir: PathBuf,
    pub fulltext_dir: PathBuf,
    pub metadata_db: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchConfig {
    pub default_top_k: usize,
    pub min_score: f32,
    pub rrf_k: usize, // RRF parameter for hybrid search
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexingConfig {
    pub chunk_size: usize,
    pub chunk_overlap: usize,
    pub batch_size: usize,
    pub supported_extensions: Vec<String>,
    pub ignore_patterns: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            embedding: EmbeddingConfig {
                provider: EmbeddingProvider::OpenAI,
                api_key: None,
                model: "text-embedding-3-small".to_string(),
                base_url: None,
            },
            storage: StorageConfig {
                data_dir: PathBuf::from("./data"),
                vectors_dir: PathBuf::from("./data/vectors"),
                fulltext_dir: PathBuf::from("./data/fulltext"),
                metadata_db: PathBuf::from("./data/metadata.db"),
            },
            search: SearchConfig {
                default_top_k: 10,
                min_score: 0.3,
                rrf_k: 100,
            },
            indexing: IndexingConfig {
                chunk_size: 1000,
                chunk_overlap: 200,
                batch_size: 100,
                supported_extensions: crate::types::Language::supported_extensions(),
                ignore_patterns: vec![],
            },
        }
    }
}

impl Config {
    /// Load configuration from environment variables and .env file
    pub fn from_env() -> Result<Self> {
        dotenv::dotenv().ok();
        
        let mut config = Self::default();
        
        // Override with environment variables
        if let Ok(provider) = std::env::var("EMBEDDING_PROVIDER") {
            config.embedding.provider = match provider.to_lowercase().as_str() {
                "openai" => EmbeddingProvider::OpenAI,
                "ollama" => EmbeddingProvider::Ollama,
                _ => EmbeddingProvider::OpenAI,
            };
        }
        
        if let Ok(api_key) = std::env::var("OPENAI_API_KEY") {
            config.embedding.api_key = Some(api_key);
        }
        
        if let Ok(model) = std::env::var("EMBEDDING_MODEL") {
            config.embedding.model = model;
        }
        
        if let Ok(base_url) = std::env::var("EMBEDDING_BASE_URL") {
            config.embedding.base_url = Some(base_url);
        }
        
        // Storage configuration
        if let Ok(data_dir) = std::env::var("DATA_DIR") {
            let data_path = PathBuf::from(data_dir);
            config.storage.data_dir = data_path.clone();
            config.storage.vectors_dir = data_path.join("vectors");
            config.storage.fulltext_dir = data_path.join("fulltext");
            config.storage.metadata_db = data_path.join("metadata.db");
        }
        
        Ok(config)
    }
}
