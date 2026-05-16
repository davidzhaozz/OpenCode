//! Shared filter for codebase walks. Excludes well-known bloat directories
//! (node_modules, target, .venv, etc.) even when they aren't in `.gitignore`,
//! so OpenCode doesn't index third-party deps as if they were the user's code.
//!
//! These are conservative: every entry here is either a generated artifact,
//! a virtualenv, a cache, or a dependency tree.

use ignore::DirEntry;

/// Skip directories with these names (anywhere in the tree).
fn is_excluded_dir(name: &str) -> bool {
    matches!(
        name,
        // JS/TS
        "node_modules" | "bower_components" | ".next" | ".nuxt" | ".svelte-kit"
        | ".turbo" | ".parcel-cache" | ".yarn"
        // Rust
        | "target"
        // Python
        | ".venv" | "venv" | "__pycache__" | ".pytest_cache" | ".mypy_cache"
        | ".tox" | ".eggs" | ".ruff_cache"
        // Go
        | "vendor"
        // Java/Kotlin/Android
        | ".gradle" | ".idea"
        // Generic build/dist
        | "dist" | "build" | "out" | ".cache" | ".dart_tool" | "DerivedData"
        // CI / coverage
        | "coverage" | ".nyc_output"
    )
}

/// Filter to hand to `ignore::WalkBuilder::filter_entry`. Drops excluded
/// directories (so the walker never recurses into them) and keeps all files.
pub fn entry_filter(e: &DirEntry) -> bool {
    let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
    if !is_dir { return true; }
    let Some(name) = e.path().file_name().and_then(|n| n.to_str()) else { return true; };
    !is_excluded_dir(name)
}
