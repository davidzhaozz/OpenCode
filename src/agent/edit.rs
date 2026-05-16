use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::diff;
use crate::llm::{openai, ChatMessage, GenOpts};

use super::system_preamble;

pub async fn run(
    cfg: &Config,
    repo: &Path,
    file: &Path,
    instruction: &str,
    yes: bool,
) -> Result<()> {
    let full_path: PathBuf = if file.is_absolute() {
        file.to_path_buf()
    } else {
        repo.join(file)
    };
    let original = std::fs::read_to_string(&full_path)
        .with_context(|| format!("reading {}", full_path.display()))?;

    let backend = openai::build(cfg);
    let msgs = vec![
        ChatMessage::system(system_preamble(cfg)),
        ChatMessage::user(format!(
            "Here is the current contents of {}:\n\n```\n{}\n```\n\n\
             Apply this change: {}\n\n\
             Respond with the FULL updated file contents only — no commentary, \
             no markdown fences, no diff format. Preserve formatting and existing \
             code that doesn't need to change.",
            file.display(),
            original,
            instruction,
        )),
    ];
    let proposed_raw = backend.chat(&msgs, &GenOpts::default()).await?.content;
    let proposed = strip_fences(&proposed_raw);

    if proposed.trim() == original.trim() {
        println!("(model returned no changes)");
        return Ok(());
    }

    diff::print_diff(&file.display().to_string(), &original, &proposed);

    let apply = yes || diff::confirm("Apply this change?")?;
    if !apply {
        eprintln!("aborted; file unchanged");
        return Ok(());
    }
    std::fs::write(&full_path, proposed)
        .with_context(|| format!("writing {}", full_path.display()))?;
    eprintln!("wrote {}", full_path.display());
    Ok(())
}

/// Small models often wrap output in ```language fences despite being told not to.
/// Strip a single leading/trailing fence if present.
fn strip_fences(s: &str) -> String {
    let trimmed = s.trim();
    if let Some(rest) = trimmed.strip_prefix("```") {
        let after_lang = rest.find('\n').map(|i| &rest[i + 1..]).unwrap_or(rest);
        if let Some(body) = after_lang.strip_suffix("```") {
            return body.trim_end().to_string() + "\n";
        }
        if let Some(end) = after_lang.rfind("```") {
            return after_lang[..end].trim_end().to_string() + "\n";
        }
    }
    s.to_string()
}

