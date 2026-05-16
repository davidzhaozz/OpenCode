//! Embeddings client. Talks to any OpenAI-compatible `/v1/embeddings` endpoint —
//! Ollama, Together, OpenAI, etc. — using the same base_url as the chat backend.
//!
//! Results are cached in the sqlite cache by (model, sha256(text)) so repeat
//! queries and re-indexing the same file contents are free.

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

use crate::cache::{content_hash, Cache};
use crate::config::Config;

pub struct EmbeddingsClient {
    base_url: String,
    api_key: Option<String>,
    model: String,
    client: reqwest::Client,
}

impl EmbeddingsClient {
    pub fn from_config(cfg: &Config) -> Self {
        Self {
            base_url: cfg.backend.base_url.trim_end_matches('/').to_string(),
            api_key: cfg.backend.api_key.clone(),
            model: cfg.embedding_model.clone(),
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .expect("reqwest client"),
        }
    }

    pub fn model(&self) -> &str { &self.model }

    /// Embed a single text. Convenience over `embed`.
    pub async fn embed_one(&self, text: &str) -> Result<Vec<f32>> {
        let mut v = self.embed(&[text.to_string()]).await?;
        v.pop().ok_or_else(|| anyhow!("empty embedding response"))
    }

    /// Embed a batch of texts. Returns vectors in the same order.
    pub async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() { return Ok(Vec::new()); }

        #[derive(Serialize)]
        struct Req<'a> { model: &'a str, input: &'a [String] }
        #[derive(Deserialize)]
        struct Resp { data: Vec<Item> }
        #[derive(Deserialize)]
        struct Item { embedding: Vec<f32>, index: usize }

        let url = format!("{}/embeddings", self.base_url);
        let mut req = self.client.post(&url).json(&Req { model: &self.model, input: texts });
        if let Some(k) = &self.api_key { req = req.bearer_auth(k); }

        let resp = req.send().await.context("embeddings request failed")?;
        let status = resp.status();
        let body = resp.text().await.context("reading embeddings response")?;
        if !status.is_success() {
            // Specific hint for the common "model not pulled" case on Ollama.
            if body.contains("not found") || body.contains("not_found") {
                return Err(anyhow!(
                    "embedding model '{}' not available on backend. \
                     If using Ollama, run: ollama pull {}",
                    self.model, self.model
                ));
            }
            return Err(anyhow!("embeddings backend returned {}: {}", status, body));
        }
        let parsed: Resp = serde_json::from_str(&body)
            .with_context(|| format!("parsing embeddings response: {body}"))?;

        // Re-sort by index in case the server returns out of order.
        let mut by_index: Vec<(usize, Vec<f32>)> =
            parsed.data.into_iter().map(|i| (i.index, i.embedding)).collect();
        by_index.sort_by_key(|(i, _)| *i);
        Ok(by_index.into_iter().map(|(_, v)| v).collect())
    }

    /// Embed with cache: only hits the backend for texts whose hash isn't cached.
    pub async fn embed_cached(&self, cache: &Cache, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let mut hashes: Vec<String> = texts.iter().map(|t| content_hash(t)).collect();
        let mut results: Vec<Option<Vec<f32>>> = vec![None; texts.len()];
        let mut to_fetch_indices: Vec<usize> = Vec::new();
        let mut to_fetch_texts: Vec<String> = Vec::new();

        for (i, h) in hashes.iter().enumerate() {
            if let Some(v) = cache.get_embedding(&self.model, h) {
                results[i] = Some(v);
            } else {
                to_fetch_indices.push(i);
                to_fetch_texts.push(texts[i].clone());
            }
        }

        if !to_fetch_texts.is_empty() {
            let fetched = self.embed(&to_fetch_texts).await?;
            for (slot, vec) in to_fetch_indices.iter().zip(fetched) {
                let _ = cache.put_embedding(&self.model, &hashes[*slot], &vec);
                results[*slot] = Some(vec);
            }
        }

        // SAFETY: every slot is filled by either cache or fetch.
        let _ = std::mem::take(&mut hashes);
        Ok(results.into_iter().map(|o| o.expect("filled above")).collect())
    }
}

/// Cosine similarity. Returns 0.0 if either vector has zero norm.
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let len = a.len().min(b.len());
    let mut dot = 0.0f32;
    let mut na = 0.0f32;
    let mut nb = 0.0f32;
    for i in 0..len {
        dot += a[i] * b[i];
        na += a[i] * a[i];
        nb += b[i] * b[i];
    }
    if na == 0.0 || nb == 0.0 { 0.0 } else { dot / (na.sqrt() * nb.sqrt()) }
}
