use anyhow::Result;
use ignore::WalkBuilder;
use std::path::{Path, PathBuf};

use super::Chunk;

/// Files we'll index. Conservative allowlist by extension keeps binaries out
/// even in repos that don't have a .gitignore.
const CODE_EXTS: &[&str] = &[
    "rs", "py", "js", "jsx", "ts", "tsx", "go", "java", "kt", "swift",
    "c", "h", "cpp", "hpp", "cc", "hh", "cs", "rb", "php", "scala",
    "sh", "bash", "zsh", "fish", "lua", "r", "jl", "ex", "exs", "erl",
    "html", "css", "scss", "vue", "svelte", "toml", "yaml", "yml",
    "json", "md", "sql",
];

const CHUNK_LINES: usize = 60;
const CHUNK_OVERLAP: usize = 10;
/// Skip files larger than this (bytes) — generated bundles, lockfiles, etc.
const MAX_FILE_BYTES: u64 = 512 * 1024;

pub fn scan_repo(root: &Path) -> Result<Vec<Chunk>> {
    let mut chunks = Vec::new();
    let walker = WalkBuilder::new(root)
        .standard_filters(true)
        .hidden(true)
        .filter_entry(crate::walk::entry_filter)
        .build();

    for entry in walker.flatten() {
        let path = entry.path();
        if !path.is_file() { continue; }
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !CODE_EXTS.contains(&ext) { continue; }
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        if meta.len() > MAX_FILE_BYTES { continue; }

        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(_) => continue, // skip binary / non-utf8
        };
        chunk_file(path.to_path_buf(), &text, &mut chunks);
    }
    Ok(chunks)
}

fn chunk_file(path: PathBuf, text: &str, out: &mut Vec<Chunk>) {
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() { return; }
    let mut start = 0;
    while start < lines.len() {
        let end = (start + CHUNK_LINES).min(lines.len());
        let body = lines[start..end].join("\n");
        out.push(Chunk {
            path: path.clone(),
            start_line: start + 1,
            end_line: end,
            text: body,
        });
        if end == lines.len() { break; }
        start = end.saturating_sub(CHUNK_OVERLAP);
        if start <= out.last().map(|c| c.start_line - 1).unwrap_or(0) && start + CHUNK_LINES <= end {
            // Defensive: never go backwards forever.
            start = end;
        }
    }
}
