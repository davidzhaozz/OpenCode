use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::diff;
use crate::llm::{openai, ChatMessage, GenOpts};

use super::system_preamble;

#[derive(Debug, Deserialize)]
struct PlanFile {
    path: String,
    contents: String,
}

#[derive(Debug, Deserialize)]
struct Plan {
    files: Vec<PlanFile>,
}

pub async fn run(cfg: &Config, out: &Path, spec: &str, yes: bool) -> Result<()> {
    if out.exists() {
        let is_empty = std::fs::read_dir(out)?.next().is_none();
        if !is_empty {
            return Err(anyhow!(
                "target directory {} exists and is not empty",
                out.display()
            ));
        }
    }

    let backend = openai::build(cfg);
    let lang = cfg.language.as_deref().unwrap_or("the most appropriate language");
    let msgs = vec![
        ChatMessage::system(system_preamble(cfg)),
        ChatMessage::user(format!(
            "Generate a complete project for this spec, written in {lang}:\n\n{spec}\n\n\
             Respond as JSON with this exact schema:\n\
             {{\"files\": [{{\"path\": \"relative/path.ext\", \"contents\": \"file body\"}}]}}\n\n\
             Rules:\n\
             - Paths must be relative (no leading /).\n\
             - Include all files needed to build and run (manifest, source, README).\n\
             - Keep it minimal and runnable; do not invent dependencies that aren't standard.\n\
             - Do NOT wrap the JSON in markdown fences."
        )),
    ];

    let raw = backend
        .chat(&msgs, &GenOpts { json_mode: true, ..Default::default() })
        .await?;
    let json = extract_json(&raw)?;
    let plan: Plan = serde_json::from_str(&json)
        .with_context(|| format!("parsing scaffold plan: {}", json))?;

    if plan.files.is_empty() {
        return Err(anyhow!("model returned an empty plan"));
    }

    println!("Plan ({} files):", plan.files.len());
    for f in &plan.files {
        let safe = sanitize_rel(&f.path)?;
        println!(
            "  {}  ({} bytes)",
            out.join(&safe).display(),
            f.contents.len()
        );
    }

    let apply = yes || diff::confirm("Write these files?")?;
    if !apply {
        eprintln!("aborted; nothing written");
        return Ok(());
    }

    std::fs::create_dir_all(out)?;
    for f in &plan.files {
        let safe = sanitize_rel(&f.path)?;
        let dest = out.join(&safe);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&dest, &f.contents)
            .with_context(|| format!("writing {}", dest.display()))?;
        eprintln!("wrote {}", dest.display());
    }
    Ok(())
}

/// Refuse absolute paths and any `..` segment to prevent writing outside `out/`.
fn sanitize_rel(p: &str) -> Result<PathBuf> {
    let path = Path::new(p);
    if path.is_absolute() {
        return Err(anyhow!("plan contains absolute path: {p}"));
    }
    for comp in path.components() {
        use std::path::Component;
        match comp {
            Component::Normal(_) | Component::CurDir => {}
            _ => return Err(anyhow!("plan contains unsafe path component in: {p}")),
        }
    }
    Ok(path.to_path_buf())
}

/// json_mode usually returns clean JSON, but some servers prepend prose.
/// Extract the first {...} block.
fn extract_json(s: &str) -> Result<String> {
    let bytes = s.as_bytes();
    let start = bytes.iter().position(|&b| b == b'{')
        .ok_or_else(|| anyhow!("no JSON object found in response: {s}"))?;
    let mut depth = 0i32;
    let mut in_str = false;
    let mut escape = false;
    for (i, &b) in bytes.iter().enumerate().skip(start) {
        if in_str {
            if escape { escape = false; }
            else if b == b'\\' { escape = true; }
            else if b == b'"' { in_str = false; }
            continue;
        }
        match b {
            b'"' => in_str = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Ok(s[start..=i].to_string());
                }
            }
            _ => {}
        }
    }
    Err(anyhow!("unterminated JSON in response: {s}"))
}
