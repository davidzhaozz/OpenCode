pub mod bm25;
pub mod hybrid;
pub mod scan;

use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Chunk {
    pub path: PathBuf,
    /// 1-indexed inclusive line range
    pub start_line: usize,
    pub end_line: usize,
    pub text: String,
}

pub trait Retriever {
    fn search(&self, query: &str, k: usize) -> Vec<(Chunk, f32)>;
}
