//! Tools exposed to the model in chat mode. Each tool has:
//! - a JSON schema describing its parameters (sent to the model)
//! - an async `run` fn that takes parsed arguments and returns a result string
//!
//! Permission gating happens inside each tool that needs it (write, edit, bash).
//! Read-only tools (read_file, list_dir, grep, find_symbol, semantic_search)
//! always auto-allow.

pub mod bash;
pub mod edit_file;
pub mod find_symbol;
pub mod grep;
pub mod list_dir;
pub mod read_file;
pub mod semantic_search;
pub mod write_file;

use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;

use crate::cache::Cache;
use crate::rag::hybrid::HybridRetriever;
use crate::symbols::SymbolIndex;

pub struct ToolCtx {
    pub repo: PathBuf,
    pub auto_yes: bool,
    pub cache: Arc<Cache>,
    pub symbols: Arc<SymbolIndex>,
    pub retriever: Arc<HybridRetriever>,
}

/// Build the OpenAI-style tools array to send with chat requests.
pub fn definitions() -> Vec<Value> {
    vec![
        function_def(
            "read_file",
            "Read the contents of a text file. Use this BEFORE editing any file. \
             Returns the file contents prefixed with line numbers. Optionally \
             read a slice via `offset` (1-indexed line) and `limit` (lines). \
             Results are cached by mtime, so re-reads are instant.",
            json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Path, absolute or relative to repo root"},
                    "offset": {"type": "integer", "description": "First line (1-indexed). Optional."},
                    "limit": {"type": "integer", "description": "Max lines to return. Optional, default 500."}
                },
                "required": ["path"]
            }),
        ),
        function_def(
            "find_symbol",
            "Look up a symbol (function, struct, class, type, etc.) by exact name. \
             Returns the file:line of each definition. **PREFER this over grep when \
             you're looking for where something is defined.** Backed by tree-sitter, \
             instant. Supports Rust, Python, JS, TS, Go.",
            json!({
                "type": "object",
                "properties": {
                    "name": {"type": "string", "description": "Exact symbol name."},
                    "kind": {"type": "string", "description": "Optional filter: fn, struct, enum, trait, type, class, def, method, function, interface, func, etc."},
                    "partial": {"type": "boolean", "description": "If true, match symbols whose name contains the query (case-insensitive). Default false."}
                },
                "required": ["name"]
            }),
        ),
        function_def(
            "semantic_search",
            "Hybrid BM25 + dense-embedding search across the repo. **Use this for \
             conceptual queries** like 'retry logic', 'auth flow', 'how is X persisted' \
             where keyword matching alone misses. Returns top-k code chunks with file:line ranges.",
            json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "Natural-language or keyword query."},
                    "k": {"type": "integer", "description": "Number of chunks to return (default 6, max 20)."}
                },
                "required": ["query"]
            }),
        ),
        function_def(
            "list_dir",
            "List entries in a directory. Returns names with 'd' or 'f' prefix to indicate dir or file.",
            json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Directory path. Defaults to repo root if omitted."}
                }
            }),
        ),
        function_def(
            "grep",
            "Search the repo for a regex pattern. Honors .gitignore. \
             Returns up to 100 matches as `path:line: text`. \
             Use for strings, comments, anything not a defined symbol — for definitions use find_symbol.",
            json!({
                "type": "object",
                "properties": {
                    "pattern": {"type": "string", "description": "Regex pattern (Rust regex syntax)"},
                    "path": {"type": "string", "description": "Directory to search. Defaults to repo root."},
                    "glob": {"type": "string", "description": "Optional file-extension filter, e.g. 'rs' or 'py'."}
                },
                "required": ["pattern"]
            }),
        ),
        function_def(
            "edit_file",
            "Replace an EXACT substring in a file. `old_string` must appear exactly once. \
             Shows a diff and asks the user to confirm before applying. \
             Use this for targeted changes; prefer it over write_file when modifying existing files.",
            json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"},
                    "old_string": {"type": "string", "description": "Exact text to find. Must be unique in the file."},
                    "new_string": {"type": "string", "description": "Replacement text."}
                },
                "required": ["path", "old_string", "new_string"]
            }),
        ),
        function_def(
            "write_file",
            "Create a new file or overwrite an existing one. Shows a diff against the previous \
             contents (or 'new file' marker) and asks the user to confirm. \
             Prefer edit_file for small changes to existing files.",
            json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"},
                    "contents": {"type": "string"}
                },
                "required": ["path", "contents"]
            }),
        ),
        function_def(
            "bash",
            "Run a shell command via `bash -c` in the repo directory. \
             Asks the user to confirm before execution. \
             Returns stdout, stderr, and exit code. Use for build, tests, formatters, git, ls, etc.",
            json!({
                "type": "object",
                "properties": {
                    "cmd": {"type": "string", "description": "Shell command line."},
                    "timeout_secs": {"type": "integer", "description": "Max runtime (default 60)."}
                },
                "required": ["cmd"]
            }),
        ),
    ]
}

fn function_def(name: &str, description: &str, parameters: Value) -> Value {
    json!({
        "type": "function",
        "function": {
            "name": name,
            "description": description,
            "parameters": parameters
        }
    })
}

/// Dispatch a tool call by name. Returns the text to send back to the model as
/// the `tool` message content.
pub async fn dispatch(name: &str, args_json: &str, ctx: &ToolCtx) -> Result<String> {
    let args: Value = serde_json::from_str(args_json)
        .map_err(|e| anyhow!("invalid JSON arguments for {name}: {e}\nargs: {args_json}"))?;
    match name {
        "read_file"        => read_file::run(&args, ctx).await,
        "find_symbol"      => find_symbol::run(&args, ctx).await,
        "semantic_search"  => semantic_search::run(&args, ctx).await,
        "list_dir"         => list_dir::run(&args, ctx).await,
        "grep"             => grep::run(&args, ctx).await,
        "edit_file"        => edit_file::run(&args, ctx).await,
        "write_file"       => write_file::run(&args, ctx).await,
        "bash"             => bash::run(&args, ctx).await,
        other => Err(anyhow!("unknown tool: {other}")),
    }
}

/// Resolve a (possibly relative) path against the repo root.
pub fn resolve(ctx: &ToolCtx, p: &str) -> PathBuf {
    let path = PathBuf::from(p);
    if path.is_absolute() { path } else { ctx.repo.join(path) }
}
