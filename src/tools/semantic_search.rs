use anyhow::{anyhow, Result};
use serde_json::Value;

use super::ToolCtx;

const DEFAULT_K: usize = 6;
const MAX_K: usize = 20;

pub async fn run(args: &Value, ctx: &ToolCtx) -> Result<String> {
    let query = args.get("query").and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("semantic_search: missing 'query'"))?;
    let k = args.get("k").and_then(|v| v.as_u64()).unwrap_or(DEFAULT_K as u64) as usize;
    let k = k.clamp(1, MAX_K);

    let hits = ctx.retriever.search(query, k).await;
    if hits.is_empty() {
        return Ok("(no relevant chunks found)".to_string());
    }
    let mut out = String::new();
    for (chunk, score) in &hits {
        let rel = chunk.path.strip_prefix(&ctx.repo).unwrap_or(&chunk.path);
        out.push_str(&format!(
            "--- [{:.3}] {}:{}-{} ---\n{}\n\n",
            score, rel.display(), chunk.start_line, chunk.end_line, chunk.text
        ));
    }
    Ok(out)
}
