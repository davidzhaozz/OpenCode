use anyhow::{anyhow, Result};
use serde_json::Value;

use super::ToolCtx;

const MAX_RESULTS: usize = 50;

pub async fn run(args: &Value, ctx: &ToolCtx) -> Result<String> {
    let name = args.get("name").and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("find_symbol: missing 'name'"))?;
    // Treat empty string as "no filter" — small models often emit "" for omitted params.
    let kind_filter = args.get("kind").and_then(|v| v.as_str()).filter(|s| !s.is_empty());
    let partial = args.get("partial").and_then(|v| v.as_bool()).unwrap_or(false);

    let syms = if partial {
        ctx.symbols.lookup_partial(name)
    } else {
        ctx.symbols.lookup_exact(name)
    };

    let filtered: Vec<_> = syms
        .into_iter()
        .filter(|s| kind_filter.map_or(true, |k| s.kind == k))
        .take(MAX_RESULTS)
        .collect();

    if filtered.is_empty() {
        return Ok(format!(
            "(no symbol named {name:?}{}. Try partial=true, or use grep for free-form search.)",
            kind_filter.map(|k| format!(" of kind {k:?}")).unwrap_or_default(),
        ));
    }

    let mut out = String::new();
    for s in &filtered {
        out.push_str(&format!("{}: {} ({}) at {}:{}\n", s.kind, s.name, s.kind, s.path.display(), s.line));
    }
    Ok(out)
}
