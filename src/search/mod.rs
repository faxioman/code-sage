
pub mod bm25;
pub mod hybrid;

pub use bm25::BM25Search;
pub use hybrid::HybridSearch;

#[derive(Debug, Clone)]
pub struct BM25Document {
    pub id: String,
    pub content: String,
    pub file_path: String,
    pub start_line: u64,
    pub end_line: u64,
}

#[derive(Debug, Clone)]
pub struct BM25Result {
    pub id: String,
    pub score: f32,
}
