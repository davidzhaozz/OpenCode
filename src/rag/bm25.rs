use std::collections::HashMap;

use super::{Chunk, Retriever};

const K1: f32 = 1.5;
const B: f32 = 0.75;

pub struct Bm25Index {
    chunks: Vec<Chunk>,
    /// Per-chunk term frequencies after tokenization.
    tf: Vec<HashMap<String, u32>>,
    /// Document frequency per term across the corpus.
    df: HashMap<String, u32>,
    /// Length of each chunk in tokens.
    lengths: Vec<u32>,
    avgdl: f32,
    n: f32,
}

impl Bm25Index {
    pub fn build(chunks: Vec<Chunk>) -> Self {
        let mut tf = Vec::with_capacity(chunks.len());
        let mut df: HashMap<String, u32> = HashMap::new();
        let mut lengths = Vec::with_capacity(chunks.len());

        for chunk in &chunks {
            let tokens = tokenize(&chunk.text);
            lengths.push(tokens.len() as u32);
            let mut local: HashMap<String, u32> = HashMap::new();
            for tok in &tokens {
                *local.entry(tok.clone()).or_default() += 1;
            }
            for term in local.keys() {
                *df.entry(term.clone()).or_default() += 1;
            }
            tf.push(local);
        }

        let n = chunks.len() as f32;
        let avgdl = if chunks.is_empty() {
            0.0
        } else {
            lengths.iter().map(|l| *l as f32).sum::<f32>() / n
        };

        Self { chunks, tf, df, lengths, avgdl, n }
    }

    fn score(&self, query_terms: &[String], i: usize) -> f32 {
        let dl = self.lengths[i] as f32;
        if dl == 0.0 { return 0.0; }
        let denom_norm = K1 * (1.0 - B + B * dl / self.avgdl.max(1.0));
        let tf = &self.tf[i];
        let mut score = 0.0;
        for term in query_terms {
            let f = *tf.get(term).unwrap_or(&0) as f32;
            if f == 0.0 { continue; }
            let df = *self.df.get(term).unwrap_or(&0) as f32;
            // Okapi BM25 IDF with +1 smoothing to keep it positive.
            let idf = (((self.n - df + 0.5) / (df + 0.5)) + 1.0).ln();
            score += idf * (f * (K1 + 1.0)) / (f + denom_norm);
        }
        score
    }
}

impl Retriever for Bm25Index {
    fn search(&self, query: &str, k: usize) -> Vec<(Chunk, f32)> {
        let query_terms = tokenize(query);
        if query_terms.is_empty() || self.chunks.is_empty() { return Vec::new(); }
        let mut scored: Vec<(usize, f32)> = (0..self.chunks.len())
            .map(|i| (i, self.score(&query_terms, i)))
            .filter(|(_, s)| *s > 0.0)
            .collect();
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(k);
        scored.into_iter().map(|(i, s)| (self.chunks[i].clone(), s)).collect()
    }
}

/// Code-aware tokenizer: splits on non-alphanumerics AND camelCase / snake_case
/// boundaries so `parseJsonResponse` matches a query for `parse json`.
fn tokenize(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut prev_lower = false;

    for ch in text.chars() {
        if ch.is_alphanumeric() {
            if ch.is_ascii_uppercase() && prev_lower && !current.is_empty() {
                flush(&mut current, &mut out);
            }
            current.push(ch.to_ascii_lowercase());
            prev_lower = ch.is_ascii_lowercase() || ch.is_ascii_digit();
        } else {
            flush(&mut current, &mut out);
            prev_lower = false;
        }
    }
    flush(&mut current, &mut out);
    out.retain(|t| t.len() > 1);
    out
}

fn flush(current: &mut String, out: &mut Vec<String>) {
    if !current.is_empty() {
        out.push(std::mem::take(current));
    }
}
