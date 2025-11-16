
use super::{BM25Result};
use crate::vectordb::SearchResult as VectorResult;
use std::collections::HashMap;

#[derive(Clone)]
pub struct HybridSearch {
    rrf_k: usize,
}

impl HybridSearch {
    pub fn new(rrf_k: usize) -> Self {
        Self { rrf_k }
    }
    
    pub fn rerank(
        &self,
        vector_results: Vec<VectorResult>,
        bm25_results: Vec<BM25Result>,
    ) -> Vec<(String, f32)> {
        let mut scores: HashMap<String, f32> = HashMap::new();
        
        for (rank, result) in vector_results.iter().enumerate() {
            let rrf_score = 1.0 / (self.rrf_k + rank + 1) as f32;
            *scores.entry(result.id.clone()).or_insert(0.0) += rrf_score;
        }
        
        for (rank, result) in bm25_results.iter().enumerate() {
            let rrf_score = 1.0 / (self.rrf_k + rank + 1) as f32;
            *scores.entry(result.id.clone()).or_insert(0.0) += rrf_score;
        }
        
        let mut results: Vec<(String, f32)> = scores.into_iter().collect();
        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        
        results
    }
    
    pub fn search(
        &self,
        vector_results: Vec<VectorResult>,
        bm25_results: Vec<BM25Result>,
        top_k: usize,
    ) -> Vec<(String, f32)> {
        let mut all_results = self.rerank(vector_results, bm25_results);
        all_results.truncate(top_k);
        all_results
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_rrf_reranking() {
        let hybrid = HybridSearch::new(100);
        
        let vector_results = vec![
            VectorResult { id: "doc1".to_string(), score: 0.9 },
            VectorResult { id: "doc2".to_string(), score: 0.8 },
            VectorResult { id: "doc3".to_string(), score: 0.7 },
        ];
        
        let bm25_results = vec![
            BM25Result { id: "doc2".to_string(), score: 10.0 },
            BM25Result { id: "doc1".to_string(), score: 9.0 },
            BM25Result { id: "doc4".to_string(), score: 8.0 },
        ];
        
        let results = hybrid.rerank(vector_results, bm25_results);
        
        assert!(results.len() >= 2);
        assert!(results[0].0 == "doc1" || results[0].0 == "doc2");
        assert!(results[1].0 == "doc1" || results[1].0 == "doc2");
    }
    
    #[test]
    fn test_rrf_score_calculation() {
        let hybrid = HybridSearch::new(100);
        
        let vector_results = vec![
            VectorResult { id: "doc1".to_string(), score: 1.0 },
        ];
        
        let bm25_results = vec![
            BM25Result { id: "doc1".to_string(), score: 100.0 },
        ];
        
        let results = hybrid.rerank(vector_results, bm25_results);
        
        assert_eq!(results[0].0, "doc1");
        assert!((results[0].1 - 2.0/101.0).abs() < 0.001);
    }
}
