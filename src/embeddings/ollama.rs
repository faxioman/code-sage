
use crate::{Error, Result};
use super::EmbeddingProvider;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
struct EmbedRequest {
    model: String,
    input: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct EmbedResponse {
    embeddings: Vec<Vec<f32>>,
}

pub struct OllamaEmbedding {
    client: reqwest::Client,
    base_url: String,
    model: String,
    dimension: usize,
    max_tokens: usize,
}

impl OllamaEmbedding {
    pub fn new(base_url: Option<String>, model: Option<String>) -> Self {
        let model_name = model.unwrap_or_else(|| "nomic-embed-text".to_string());
        let base_url = base_url.unwrap_or_else(|| "http://127.0.0.1:11434".to_string());
        
        let dimension = 768;
        let max_tokens = Self::default_max_tokens_for_model(&model_name);
        
        Self {
            client: reqwest::Client::new(),
            base_url,
            model: model_name,
            dimension,
            max_tokens,
        }
    }
    
    fn default_max_tokens_for_model(model: &str) -> usize {
        if model.contains("nomic-embed-text") || model.contains("snowflake-arctic-embed") {
            8192
        } else {
            2048
        }
    }
    
    pub async fn initialize(&mut self) -> Result<()> {
        let test_embedding = self.embed("test").await?;
        self.dimension = test_embedding.len();
        Ok(())
    }
    
    fn preprocess_text(&self, text: &str) -> String {
        if text.is_empty() {
            return " ".to_string();
        }
        
        let max_chars = self.max_tokens * 4;
        if text.len() > max_chars {
            text.chars().take(max_chars).collect()
        } else {
            text.to_string()
        }
    }
    
    fn preprocess_texts(&self, texts: &[String]) -> Vec<String> {
        texts.iter()
            .map(|t| self.preprocess_text(t))
            .collect()
    }
}

#[async_trait]
impl EmbeddingProvider for OllamaEmbedding {
    async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let processed = self.preprocess_text(text);
        
        let request = EmbedRequest {
            model: self.model.clone(),
            input: serde_json::Value::String(processed),
        };
        
        let url = format!("{}/api/embed", self.base_url);
        
        let response = self.client
            .post(&url)
            .json(&request)
            .send()
            .await
            .map_err(|e| Error::Embedding(format!("Ollama error: {e}")))?;
        
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(Error::Embedding(format!("Ollama API error {status}: {body}")));
        }
        
        let embed_response: EmbedResponse = response.json().await
            .map_err(|e| Error::Embedding(format!("Ollama JSON parse error: {e}")))?;
        
        embed_response.embeddings.into_iter().next()
            .ok_or_else(|| Error::Embedding("Empty response".to_string()))
    }
    
    async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let processed = self.preprocess_texts(texts);
        
        let request = EmbedRequest {
            model: self.model.clone(),
            input: serde_json::Value::Array(
                processed.into_iter().map(serde_json::Value::String).collect()
            ),
        };
        
        let url = format!("{}/api/embed", self.base_url);
        
        let response = self.client
            .post(&url)
            .json(&request)
            .send()
            .await
            .map_err(|e| Error::Embedding(format!("Ollama batch error: {e}")))?;
        
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(Error::Embedding(format!("Ollama API error {status}: {body}")));
        }
        
        let embed_response: EmbedResponse = response.json().await
            .map_err(|e| Error::Embedding(format!("Ollama JSON parse error: {e}")))?;
        
        Ok(embed_response.embeddings)
    }
    
    fn dimension(&self) -> usize {
        self.dimension
    }
    
    fn provider_name(&self) -> &str {
        "Ollama"
    }
}
