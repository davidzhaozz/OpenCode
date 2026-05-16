//! Hybrid retrieval: BM25 + dense embeddings fused via Reciprocal Rank Fusion.
//!
//! BM25 handles symbol/identifier queries; embeddings handle conceptual queries
//! ("retry logic", "auth flow") where keyword matching misses. RRF is parameter-
//! free and combines rankings cleanly.

use anyhow::Result;
use std::sync::Arc;

use super::bm25::Bm25Index;
use super::{Chunk, Retriever};
use crate::cache::Cache;
use crate::llm::embeddings::{cosine, EmbeddingsClient};

const RRF_K: f32 = 60.0;

pub struct HybridRetriever {
    bm25: Bm25Index,
    /// Per-chunk embeddings. Same length as `bm25.chunks`.
    chunk_embeds: Option<Vec<Vec<f32>>>,
    client: Option<EmbeddingsClient>,
}

impl HybridRetriever {
    /// Build a hybrid index. If embeddings fail (model missing etc.), falls
    /// back to BM25-only and prints a one-line warning to stderr.
    pub async fn build(
        chunks: Vec<Chunk>,
        client: EmbeddingsClient,
        cache: Arc<Cache>,
    ) -> Self {
        let bm25 = Bm25Index::build(chunks);
        let texts: Vec<String> = bm25.chunks.iter().map(|c| c.text.clone()).collect();

        match client.embed_cached(&cache, &texts).await {
            Ok(embeds) => Self { bm25, chunk_embeds: Some(embeds), client: Some(client) },
            Err(e) => {
                eprintln!("(embeddings disabled: {e}; falling back to BM25-only)");
                Self { bm25, chunk_embeds: None, client: None }
            }
        }
    }

    pub async fn search(&self, query: &str, k: usize) -> Vec<(Chunk, f32)> {
        let bm25_top = self.bm25.search(query, (k * 4).max(20));

        let Some((embeds, client)) = self.chunk_embeds.as_ref().zip(self.client.as_ref()) else {
            return bm25_top.into_iter().take(k).collect();
        };

        let q_embed = match client.embed_one(query).await {
            Ok(v) => v,
            Err(_) => return bm25_top.into_iter().take(k).collect(),
        };

        // Score all chunks by cosine similarity to the query.
        let mut sem_scored: Vec<(usize, f32)> = embeds
            .iter()
            .enumerate()
            .map(|(i, v)| (i, cosine(v, &q_embed)))
            .collect();
        sem_scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        sem_scored.truncate((k * 4).max(20));

        // Map chunks to indices for RRF.
        let bm25_ranks: Vec<(usize, usize)> = bm25_top
            .iter()
            .enumerate()
            .map(|(rank, (chunk, _))| (chunk_index_of(&self.bm25.chunks, chunk), rank))
            .collect();
        let sem_ranks: Vec<(usize, usize)> = sem_scored
            .iter()
            .enumerate()
            .map(|(rank, (idx, _))| (*idx, rank))
            .collect();

        let fused = rrf_fuse(&bm25_ranks, &sem_ranks, k);
        fused
            .into_iter()
            .map(|(idx, score)| (self.bm25.chunks[idx].clone(), score))
            .collect()
    }
}

fn chunk_index_of(all: &[Chunk], target: &Chunk) -> usize {
    // Chunks are identified by (path, start_line) — both are set at indexing.
    all.iter()
        .position(|c| c.path == target.path && c.start_line == target.start_line)
        .unwrap_or(0)
}

/// Reciprocal Rank Fusion. Lower rank is better in inputs; output is sorted
/// descending by fused score.
fn rrf_fuse(a: &[(usize, usize)], b: &[(usize, usize)], k: usize) -> Vec<(usize, f32)> {
    use std::collections::HashMap;
    let mut scores: HashMap<usize, f32> = HashMap::new();
    for (idx, rank) in a {
        *scores.entry(*idx).or_insert(0.0) += 1.0 / (RRF_K + *rank as f32 + 1.0);
    }
    for (idx, rank) in b {
        *scores.entry(*idx).or_insert(0.0) += 1.0 / (RRF_K + *rank as f32 + 1.0);
    }
    let mut ranked: Vec<(usize, f32)> = scores.into_iter().collect();
    ranked.sort_by(|x, y| y.1.partial_cmp(&x.1).unwrap_or(std::cmp::Ordering::Equal));
    ranked.truncate(k);
    ranked
}

/// Convenience builder used by callers that don't want to manage the cache.
pub async fn build(chunks: Vec<Chunk>, cfg: &crate::config::Config, cache: Arc<Cache>) -> Result<HybridRetriever> {
    let client = EmbeddingsClient::from_config(cfg);
    Ok(HybridRetriever::build(chunks, client, cache).await)
}
