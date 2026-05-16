use anyhow::{anyhow, Result};
use serde_json::Value;

use crate::diff;

use super::{resolve, ToolCtx};

pub async fn run(args: &Value, ctx: &ToolCtx) -> Result<String> {
    let path = args.get("path").and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("write_file: missing 'path'"))?;
    let contents = args.get("contents").and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("write_file: missing 'contents'"))?;

    let full = resolve(ctx, path);
    let original = std::fs::read_to_string(&full).unwrap_or_default();
    let is_new = !full.exists();

    if !is_new && original == contents {
        return Ok(format!("(no change — contents identical for {path})"));
    }

    if is_new {
        println!("--- new file: {path} ({} bytes) ---", contents.len());
        let preview: String = contents.lines().take(30).collect::<Vec<_>>().join("\n");
        println!("{preview}");
        if contents.lines().count() > 30 {
            println!("... [{} more lines]", contents.lines().count() - 30);
        }
    } else {
        diff::print_diff(path, &original, contents);
    }

    let apply = ctx.auto_yes
        || diff::confirm(&format!("{} {path}?", if is_new { "Create" } else { "Overwrite" }))?;
    if !apply {
        return Ok(format!("user rejected write to {path}"));
    }
    if let Some(parent) = full.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| anyhow!("write_file: mkdir {}: {}", parent.display(), e))?;
    }
    std::fs::write(&full, contents)
        .map_err(|e| anyhow!("write_file: writing {}: {}", full.display(), e))?;
    Ok(format!(
        "{} {path} ({} bytes)",
        if is_new { "created" } else { "overwrote" },
        contents.len()
    ))
}
