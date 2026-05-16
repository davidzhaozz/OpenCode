//! Repo manifest: a compact tree + per-file one-line summaries that gets
//! injected into the chat system prompt so the model boots already oriented.
//!
//! Summaries come from a heuristic (first comment / docstring / non-empty line)
//! and are cached per-file by mtime in the sqlite cache.

use anyhow::Result;
use ignore::WalkBuilder;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::cache::Cache;

const CODE_EXTS: &[&str] = &[
    "rs", "py", "js", "jsx", "ts", "tsx", "go", "java", "kt", "swift",
    "c", "h", "cpp", "hpp", "cc", "hh", "cs", "rb", "php", "scala",
    "sh", "bash", "lua", "ex", "exs",
    "html", "css", "scss", "vue", "svelte",
    "toml", "yaml", "yml", "json", "md", "sql",
];
const MAX_FILES_LISTED: usize = 80;
const MAX_DEPTH: usize = 4;
const MAX_FILE_BYTES_FOR_SUMMARY: u64 = 256 * 1024;
const SUMMARY_MAX_CHARS: usize = 100;

pub struct Manifest {
    pub root: PathBuf,
    pub rendered: String,
    pub file_count: usize,
}

pub fn build(root: &Path, cache: &Cache) -> Result<Manifest> {
    let mut entries: Vec<(PathBuf, String)> = Vec::new(); // (relative path, summary)
    let walker = WalkBuilder::new(root)
        .standard_filters(true)
        .hidden(true)
        .max_depth(Some(MAX_DEPTH))
        .filter_entry(crate::walk::entry_filter)
        .build();

    for entry in walker.flatten() {
        let p = entry.path();
        if !p.is_file() { continue; }
        let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !CODE_EXTS.contains(&ext) { continue; }
        let Ok(meta) = entry.metadata() else { continue; };
        if meta.len() > MAX_FILE_BYTES_FOR_SUMMARY { continue; }
        let rel = match p.strip_prefix(root) {
            Ok(r) => r.to_path_buf(),
            Err(_) => continue,
        };
        let summary = file_summary(p, cache);
        entries.push((rel, summary));
    }

    let file_count = entries.len();
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    let rendered = render_tree(&entries);
    Ok(Manifest { root: root.to_path_buf(), rendered, file_count })
}

/// Extract or fetch the cached one-line summary for a file.
fn file_summary(path: &Path, cache: &Cache) -> String {
    if let Some(s) = cache.get_summary(path) {
        return s;
    }
    let summary = match std::fs::read_to_string(path) {
        Ok(text) => extract_summary(&text, path),
        Err(_) => String::new(),
    };
    let _ = cache.put_summary(path, &summary);
    summary
}

/// Heuristic: first doc-comment / docstring / non-empty meaningful line.
fn extract_summary(text: &str, path: &Path) -> String {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");

    // Python docstring at top: """ ... """ or ''' ... '''
    if ext == "py" {
        let trimmed = text.trim_start();
        for triple in ["\"\"\"", "'''"] {
            if let Some(rest) = trimmed.strip_prefix(triple) {
                if let Some(end) = rest.find(triple) {
                    let body = &rest[..end];
                    return clean(body.lines().next().unwrap_or(""));
                }
            }
        }
    }

    // Markdown: first heading or paragraph
    if ext == "md" {
        for line in text.lines() {
            let t = line.trim();
            if t.is_empty() { continue; }
            return clean(t.trim_start_matches('#').trim());
        }
        return String::new();
    }

    // Comment-style summaries
    for line in text.lines().take(50) {
        let t = line.trim();
        if t.is_empty() { continue; }
        let stripped = strip_comment_lead(t);
        if let Some(s) = stripped {
            if !s.is_empty() && !looks_like_attribute(s) {
                return clean(s);
            }
            continue;
        }
        // First non-empty non-comment line. Use it only if it looks declarative.
        if t.starts_with("pub ") || t.starts_with("fn ") || t.starts_with("def ")
            || t.starts_with("class ") || t.starts_with("function ") || t.starts_with("export ")
            || t.starts_with("package ") || t.starts_with("interface ")
        {
            return clean(t);
        }
        return String::new();
    }
    String::new()
}

fn strip_comment_lead(line: &str) -> Option<&str> {
    let opens = ["///", "//!", "//", "#!", "#", "/*", "*", "<!--"];
    for p in opens {
        if let Some(rest) = line.strip_prefix(p) {
            let r = rest.trim_start_matches(['/', '*', '<', '-', '!', '#']);
            return Some(r.trim());
        }
    }
    None
}

fn looks_like_attribute(s: &str) -> bool {
    let t = s.trim_start_matches('!');
    t.starts_with("[derive") || t.starts_with("[cfg") || t.starts_with("[allow")
        || t.starts_with("[serde") || t.starts_with("[arg") || t.starts_with("[command")
        || t.starts_with("[tokio")
}

fn clean(s: &str) -> String {
    let mut out: String = s.chars().take(SUMMARY_MAX_CHARS).collect();
    out = out.trim().to_string();
    // Collapse internal whitespace.
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Render the file list as an indented tree grouped by directory.
fn render_tree(entries: &[(PathBuf, String)]) -> String {
    // group by directory
    let mut by_dir: BTreeMap<PathBuf, Vec<(String, String)>> = BTreeMap::new();
    for (rel, sum) in entries {
        let dir = rel.parent().map(|p| p.to_path_buf()).unwrap_or_default();
        let name = rel
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        by_dir.entry(dir).or_default().push((name, sum.clone()));
    }

    let mut out = String::new();
    let mut listed = 0usize;
    let total = entries.len();

    for (dir, files) in &by_dir {
        let dir_label = if dir.as_os_str().is_empty() {
            "./".to_string()
        } else {
            format!("{}/", dir.display())
        };
        out.push_str(&dir_label);
        out.push('\n');
        for (name, summary) in files {
            if listed >= MAX_FILES_LISTED {
                let remaining = total - listed;
                if remaining > 0 {
                    out.push_str(&format!("  ... ({remaining} more files)\n"));
                }
                return out;
            }
            if summary.is_empty() {
                out.push_str(&format!("  {name}\n"));
            } else {
                out.push_str(&format!("  {:<24} {}\n", name, summary));
            }
            listed += 1;
        }
    }
    out
}
