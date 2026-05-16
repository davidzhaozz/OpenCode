use anyhow::{anyhow, Result};
use serde_json::Value;

use crate::diff;

use super::{resolve, ToolCtx};

pub async fn run(args: &Value, ctx: &ToolCtx) -> Result<String> {
    let path = args.get("path").and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("edit_file: missing 'path'"))?;
    let old_string = args.get("old_string").and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("edit_file: missing 'old_string'"))?;
    let new_string = args.get("new_string").and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("edit_file: missing 'new_string'"))?;

    let full = resolve(ctx, path);
    let original = std::fs::read_to_string(&full)
        .map_err(|e| anyhow!("edit_file: cannot read {}: {}", full.display(), e))?;

    let occurrences = original.matches(old_string).count();
    if occurrences == 0 {
        return Ok(format!("ERROR: old_string not found in {path}. Re-read the file to see exact contents."));
    }
    if occurrences > 1 {
        return Ok(format!(
            "ERROR: old_string matches {occurrences} times in {path}. Make old_string longer/more unique."
        ));
    }

    let updated = original.replacen(old_string, new_string, 1);
    if updated == original {
        return Ok(format!("(no change — old_string == new_string in {path})"));
    }

    diff::print_diff(path, &original, &updated);
    let apply = ctx.auto_yes || diff::confirm(&format!("Apply edit to {path}?"))?;
    if !apply {
        return Ok(format!("user rejected edit to {path}"));
    }
    std::fs::write(&full, &updated)
        .map_err(|e| anyhow!("edit_file: writing {}: {}", full.display(), e))?;
    Ok(format!("edited {path} ({} bytes -> {} bytes)", original.len(), updated.len()))
}
