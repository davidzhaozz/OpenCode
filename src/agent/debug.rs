use anyhow::{anyhow, Context, Result};
use regex::Regex;
use serde::Deserialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::diff;
use crate::exec;
use crate::llm::{openai, ChatMessage, GenOpts};

use super::system_preamble;

#[derive(Debug, Deserialize)]
struct FixFile {
    path: String,
    contents: String,
}

#[derive(Debug, Deserialize)]
struct Fix {
    /// Optional model explanation (1-2 sentences) of what it changed and why.
    #[serde(default)]
    rationale: String,
    files: Vec<FixFile>,
}

pub async fn run(
    cfg: &Config,
    repo: &Path,
    cmd: &[String],
    max_iters: usize,
    yes: bool,
) -> Result<()> {
    let backend = openai::build(cfg);

    for iter in 1..=max_iters {
        eprintln!("--- iteration {iter}/{max_iters} ---");
        eprintln!("$ {}", cmd.join(" "));
        let out = exec::run(repo, cmd).await?;
        if out.status == 0 {
            eprintln!("command succeeded.");
            if !out.stdout.is_empty() { println!("{}", out.stdout); }
            return Ok(());
        }
        eprintln!("exit {}; asking model for a fix", out.status);

        // Pull file paths hinted by stderr so we include them as context.
        let hinted = referenced_paths(&out.stderr, repo);
        let mut hinted_context = String::new();
        for p in &hinted {
            if let Ok(text) = std::fs::read_to_string(p) {
                hinted_context.push_str(&format!(
                    "--- {} ---\n{}\n\n",
                    p.strip_prefix(repo).unwrap_or(p).display(),
                    text
                ));
            }
        }

        let user_prompt = format!(
            "A command failed with exit code {}.\n\n\
             Command: {}\n\n\
             stderr (tail):\n{}\n\n\
             stdout (tail):\n{}\n\n\
             Files referenced in the error:\n{}\n\n\
             Propose a minimal fix. Respond as JSON:\n\
             {{\"rationale\": \"one or two sentences\", \
             \"files\": [{{\"path\": \"relative/path\", \"contents\": \"FULL new file contents\"}}]}}\n\
             Only include files you are actually changing. Paths must be relative to the repo root.",
            out.status,
            cmd.join(" "),
            exec::tail(&out.stderr, 4000),
            exec::tail(&out.stdout, 1000),
            if hinted_context.is_empty() { "(none detected)".into() } else { hinted_context },
        );

        let msgs = vec![
            ChatMessage::system(system_preamble(cfg)),
            ChatMessage::user(user_prompt),
        ];
        let raw = backend
            .chat(&msgs, &GenOpts { json_mode: true, ..Default::default() })
            .await?
            .content;
        let json = extract_json(&raw)?;
        let fix: Fix = serde_json::from_str(&json)
            .with_context(|| format!("parsing fix plan: {json}"))?;

        if fix.files.is_empty() {
            return Err(anyhow!("model returned no files to change; giving up"));
        }
        if !fix.rationale.trim().is_empty() {
            eprintln!("rationale: {}", fix.rationale.trim());
        }

        let mut applied_any = false;
        for f in &fix.files {
            let rel = sanitize_rel(&f.path)?;
            let dest = repo.join(&rel);
            let old = std::fs::read_to_string(&dest).unwrap_or_default();
            if old == f.contents { continue; }
            diff::print_diff(&rel.display().to_string(), &old, &f.contents);
            let apply = yes || diff::confirm(&format!("Apply change to {}?", rel.display()))?;
            if !apply {
                eprintln!("skipped {}", rel.display());
                continue;
            }
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&dest, &f.contents)
                .with_context(|| format!("writing {}", dest.display()))?;
            applied_any = true;
        }
        if !applied_any {
            eprintln!("no changes applied; stopping");
            return Ok(());
        }
    }
    Err(anyhow!("reached max iterations ({}) without success", max_iters))
}

fn sanitize_rel(p: &str) -> Result<PathBuf> {
    let path = Path::new(p);
    if path.is_absolute() {
        return Err(anyhow!("fix contains absolute path: {p}"));
    }
    for comp in path.components() {
        use std::path::Component;
        match comp {
            Component::Normal(_) | Component::CurDir => {}
            _ => return Err(anyhow!("fix contains unsafe path component in: {p}")),
        }
    }
    Ok(path.to_path_buf())
}

/// Best-effort scan of stderr for filesystem paths that exist in the repo.
fn referenced_paths(stderr: &str, repo: &Path) -> Vec<PathBuf> {
    // Matches things like src/foo.rs, foo/bar.py:42, ./baz.ts
    let re = Regex::new(r"(?m)([A-Za-z0-9_\-./]+\.[A-Za-z0-9]{1,6})(?::\d+)?").unwrap();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let mut out = Vec::new();
    for cap in re.captures_iter(stderr) {
        let raw = cap.get(1).unwrap().as_str();
        let candidate = repo.join(raw);
        if candidate.is_file() && seen.insert(candidate.clone()) {
            out.push(candidate);
        }
        if out.len() >= 5 { break; }
    }
    out
}

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
