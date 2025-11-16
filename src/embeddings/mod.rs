
pub mod openai;
pub mod ollama;

use crate::Result;
use async_trait::async_trait;

#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    async fn embed(&self, text: &str) -> Result<Vec<f32>>;
    
    async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>>;
    
    fn dimension(&self) -> usize;
    
    fn provider_name(&self) -> &str;
}

pub use openai::OpenAIEmbedding;
pub use ollama::OllamaEmbedding;
