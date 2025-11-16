//! OpenAI embedding provider

use crate::{Error, Result};
use super::EmbeddingProvider;
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};

pub struct OpenAIEmbedding {
    client: Client,
    api_key: String,
    model: String,
    base_url: String,
    dimension: usize,
    max_tokens: usize,
}

#[derive(Serialize)]
struct EmbeddingRequest {
    model: String,
    input: Vec<String>,
    encoding_format: String,
}

#[derive(Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingData>,
}

#[derive(Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
}

impl OpenAIEmbedding {
    pub fn new(api_key: String, model: Option<String>, base_url: Option<String>) -> Self {
        let model = model.unwrap_or_else(|| "text-embedding-3-small".to_string());
        let base_url = base_url.unwrap_or_else(|| "https://api.openai.com/v1".to_string());
        
        let dimension = 0;
        let max_tokens = 8192;
        
        Self {
            client: Client::new(),
            api_key,
            model,
            base_url,
            dimension,
            max_tokens,
        }
    }
    
    pub async fn detect_dimension(&mut self) -> Result<usize> {
        let test_text = vec!["test".to_string()];
        let result = self.embed_batch(&test_text).await?;
        
        if let Some(first) = result.first() {
            self.dimension = first.len();
            Ok(self.dimension)
        } else {
            Err(Error::Embedding("Failed to detect dimension".to_string()))
        }
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
impl EmbeddingProvider for OpenAIEmbedding {
    async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let result = self.embed_batch(&[text.to_string()]).await?;
        result.into_iter().next()
            .ok_or_else(|| Error::Embedding("No embedding returned".to_string()))
    }
    
    async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let processed = self.preprocess_texts(texts);
        
        let request = EmbeddingRequest {
            model: self.model.clone(),
            input: processed,
            encoding_format: "float".to_string(),
        };
        
        let url = format!("{}/embeddings", self.base_url);
        
        let response = self.client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await?;
        
        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            return Err(Error::Embedding(format!(
                "OpenAI API error ({status}): {error_text}"
            )));
        }
        
        let embedding_response: EmbeddingResponse = response.json().await?;
        
        Ok(embedding_response.data.into_iter()
            .map(|d| d.embedding)
            .collect())
    }
    
    fn dimension(&self) -> usize {
        self.dimension
    }
    
    fn provider_name(&self) -> &str {
        "OpenAI"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    #[ignore]
    async fn test_openai_embed() {
        let api_key = std::env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY not set");
        let provider = OpenAIEmbedding::new(api_key, None, None);
        
        let result = provider.embed("Hello world").await;
        assert!(result.is_ok());
        
        let embedding = result.unwrap();
        assert_eq!(embedding.len(), 1536);
    }
}
