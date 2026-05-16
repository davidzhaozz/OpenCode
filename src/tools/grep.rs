use anyhow::{anyhow, Result};
use ignore::WalkBuilder;
use regex::Regex;
use serde_json::Value;

use super::{resolve, ToolCtx};

const MAX_MATCHES: usize = 100;
const MAX_FILE_BYTES: u64 = 1024 * 1024;

pub async fn run(args: &Value, ctx: &ToolCtx) -> Result<String> {
    let pattern = args.get("pattern").and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("grep: missing 'pattern'"))?;
    let path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
    let glob_ext = args.get("glob").and_then(|v| v.as_str()).map(|s| s.trim_start_matches('.').to_string());

    let re = Regex::new(pattern).map_err(|e| anyhow!("grep: invalid regex: {e}"))?;
    let root = resolve(ctx, path);

    let walker = WalkBuilder::new(&root).standard_filters(true).hidden(true).build();
    let mut matches = Vec::new();
    let mut scanned_files = 0usize;

    'outer: for entry in walker.flatten() {
        let p = entry.path();
        if !p.is_file() { continue; }
        if let Some(ext) = &glob_ext {
            if p.extension().and_then(|e| e.to_str()) != Some(ext.as_str()) { continue; }
        }
        let meta = match entry.metadata() { Ok(m) => m, Err(_) => continue };
        if meta.len() > MAX_FILE_BYTES { continue; }

        let text = match std::fs::read_to_string(p) { Ok(t) => t, Err(_) => continue };
        scanned_files += 1;
        for (i, line) in text.lines().enumerate() {
            if re.is_match(line) {
                let rel = p.strip_prefix(&ctx.repo).unwrap_or(p);
                matches.push(format!("{}:{}: {}", rel.display(), i + 1, line.trim_end()));
                if matches.len() >= MAX_MATCHES { break 'outer; }
            }
        }
    }

    if matches.is_empty() {
        return Ok(format!("(no matches in {scanned_files} files)"));
    }
    let mut out = matches.join("\n");
    out.push('\n');
    if matches.len() == MAX_MATCHES {
        out.push_str(&format!("\n[hit {MAX_MATCHES} match cap; narrow the pattern or path]\n"));
    }
    Ok(out)
}
