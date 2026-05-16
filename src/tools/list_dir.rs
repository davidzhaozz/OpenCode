use anyhow::{anyhow, Result};
use serde_json::Value;

use super::{resolve, ToolCtx};

const MAX_ENTRIES: usize = 200;

pub async fn run(args: &Value, ctx: &ToolCtx) -> Result<String> {
    let path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
    let full = resolve(ctx, path);
    let entries = std::fs::read_dir(&full)
        .map_err(|e| anyhow!("list_dir {}: {}", full.display(), e))?;

    let mut rows: Vec<(bool, String)> = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        rows.push((is_dir, name));
    }
    rows.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));

    let total = rows.len();
    rows.truncate(MAX_ENTRIES);
    let mut out = String::new();
    for (is_dir, name) in &rows {
        out.push_str(&format!("{} {}\n", if *is_dir { "d" } else { "f" }, name));
    }
    if total > MAX_ENTRIES {
        out.push_str(&format!("\n[{total} entries; showing first {MAX_ENTRIES}]\n"));
    }
    if out.is_empty() {
        out = "(empty directory)\n".into();
    }
    Ok(out)
}
