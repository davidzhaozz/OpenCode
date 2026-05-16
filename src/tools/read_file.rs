use anyhow::{anyhow, Result};
use serde_json::Value;

use super::{resolve, ToolCtx};

const DEFAULT_LIMIT: usize = 500;
const MAX_LIMIT: usize = 2000;

pub async fn run(args: &Value, ctx: &ToolCtx) -> Result<String> {
    let path = args.get("path").and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("read_file: missing 'path'"))?;
    let offset = args.get("offset").and_then(|v| v.as_u64()).unwrap_or(1).max(1) as usize;
    let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(DEFAULT_LIMIT as u64) as usize;
    let limit = limit.min(MAX_LIMIT);

    let full = resolve(ctx, path);
    // Consult cache first: mtime-keyed file_reads. Hit = avoid disk + utf8 work.
    let text = if let Some(cached) = ctx.cache.get_file_read(&full) {
        cached
    } else {
        let t = std::fs::read_to_string(&full)
            .map_err(|e| anyhow!("read_file {}: {}", full.display(), e))?;
        let _ = ctx.cache.put_file_read(&full, &t);
        t
    };

    let total = text.lines().count();
    let start = offset.saturating_sub(1);
    let mut out = String::new();
    let mut shown = 0;
    for (i, line) in text.lines().enumerate().skip(start).take(limit) {
        out.push_str(&format!("{:>5}  {}\n", i + 1, line));
        shown += 1;
    }
    if shown == 0 {
        return Ok(format!("(file has {total} lines; offset {offset} is past the end)"));
    }
    if start + shown < total {
        out.push_str(&format!(
            "\n[showed lines {}-{} of {total}; pass offset/limit to see more]\n",
            start + 1,
            start + shown
        ));
    }
    Ok(out)
}
