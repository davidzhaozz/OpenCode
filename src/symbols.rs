//! Tree-sitter symbol index. For each source file, extract definitions
//! (functions, structs, classes, etc.) with their location. Cached per file
//! by mtime in sqlite. Enables fast `find_symbol` lookups in chat mode.

use anyhow::{anyhow, Result};
use ignore::WalkBuilder;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tree_sitter::{Language, Parser, Query, QueryCursor};

use crate::cache::Cache;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Symbol {
    pub name: String,
    pub kind: String,
    /// Relative to the repo root.
    pub path: PathBuf,
    /// 1-indexed line of the definition.
    pub line: usize,
}

#[derive(Default)]
pub struct SymbolIndex {
    pub by_name: HashMap<String, Vec<Symbol>>,
    pub by_path: HashMap<PathBuf, Vec<Symbol>>,
}

impl SymbolIndex {
    pub fn lookup_exact(&self, name: &str) -> Vec<&Symbol> {
        self.by_name.get(name).map(|v| v.iter().collect()).unwrap_or_default()
    }

    /// Case-insensitive partial match. Cheap because the by_name table is small
    /// even for large repos (one entry per unique identifier).
    pub fn lookup_partial(&self, query: &str) -> Vec<&Symbol> {
        let q = query.to_ascii_lowercase();
        let mut out = Vec::new();
        for (name, syms) in &self.by_name {
            if name.to_ascii_lowercase().contains(&q) {
                for s in syms { out.push(s); }
            }
        }
        out
    }
}

/// Build (or refresh) the symbol index for `root`. Uses the sqlite cache so
/// unchanged files are not reparsed.
pub fn build(root: &Path, cache: &Cache) -> Result<SymbolIndex> {
    let mut by_name: HashMap<String, Vec<Symbol>> = HashMap::new();
    let mut by_path: HashMap<PathBuf, Vec<Symbol>> = HashMap::new();

    let walker = WalkBuilder::new(root)
        .standard_filters(true)
        .hidden(true)
        .filter_entry(crate::walk::entry_filter)
        .build();
    for entry in walker.flatten() {
        let p = entry.path();
        if !p.is_file() { continue; }
        let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
        let Some(spec) = lang_spec_for_ext(ext) else { continue; };

        let symbols = match cache.get_symbols(p) {
            Some(blob) => match serde_json::from_str::<Vec<Symbol>>(&blob) {
                Ok(v) => v,
                Err(_) => extract_and_cache(p, root, spec, cache)?,
            },
            None => extract_and_cache(p, root, spec, cache)?,
        };

        for s in &symbols {
            by_name.entry(s.name.clone()).or_default().push(s.clone());
        }
        let rel = p.strip_prefix(root).unwrap_or(p).to_path_buf();
        by_path.insert(rel, symbols);
    }

    Ok(SymbolIndex { by_name, by_path })
}

fn extract_and_cache(
    abs_path: &Path,
    root: &Path,
    spec: &LangSpec,
    cache: &Cache,
) -> Result<Vec<Symbol>> {
    let source = match std::fs::read_to_string(abs_path) {
        Ok(s) => s,
        Err(_) => return Ok(Vec::new()),
    };
    let rel = abs_path.strip_prefix(root).unwrap_or(abs_path).to_path_buf();
    let symbols = extract(&source, &rel, spec)?;
    let blob = serde_json::to_string(&symbols).unwrap_or_default();
    let _ = cache.put_symbols(abs_path, &blob);
    Ok(symbols)
}

fn extract(source: &str, rel_path: &Path, spec: &LangSpec) -> Result<Vec<Symbol>> {
    let language = (spec.lang)();
    let mut parser = Parser::new();
    parser.set_language(&language).map_err(|e| anyhow!("set_language: {e}"))?;
    let tree = parser.parse(source, None).ok_or_else(|| anyhow!("parse failed"))?;

    let mut out: Vec<Symbol> = Vec::new();
    for (kind, query_str) in spec.queries {
        let query = match Query::new(&language, query_str) {
            Ok(q) => q,
            Err(_) => continue, // grammar version mismatch — skip this kind
        };
        // The capture we want is named "name" in every query.
        let Some(name_capture_idx) = query.capture_index_for_name("name") else { continue; };

        let mut cursor = QueryCursor::new();
        let matches = cursor.matches(&query, tree.root_node(), source.as_bytes());
        for m in matches {
            for cap in m.captures {
                if cap.index != name_capture_idx { continue; }
                let node = cap.node;
                let name = match node.utf8_text(source.as_bytes()) {
                    Ok(s) => s.to_string(),
                    Err(_) => continue,
                };
                let line = node.start_position().row + 1;
                out.push(Symbol {
                    name,
                    kind: (*kind).to_string(),
                    path: rel_path.to_path_buf(),
                    line,
                });
            }
        }
    }
    // Deduplicate (same name + line can appear under multiple kinds for nested defs).
    out.sort_by(|a, b| (a.line, &a.name).cmp(&(b.line, &b.name)));
    out.dedup_by(|a, b| a.line == b.line && a.name == b.name && a.kind == b.kind);
    Ok(out)
}

struct LangSpec {
    lang: fn() -> Language,
    queries: &'static [(&'static str, &'static str)],
}

fn lang_spec_for_ext(ext: &str) -> Option<&'static LangSpec> {
    match ext {
        "rs"           => Some(&RUST),
        "py"           => Some(&PYTHON),
        "js" | "jsx" | "mjs" | "cjs" => Some(&JAVASCRIPT),
        "ts" | "tsx"   => Some(&TYPESCRIPT),
        "go"           => Some(&GO),
        _ => None,
    }
}

static RUST: LangSpec = LangSpec {
    lang: tree_sitter_rust::language,
    queries: &[
        ("fn",     "(function_item name: (identifier) @name)"),
        ("struct", "(struct_item name: (type_identifier) @name)"),
        ("enum",   "(enum_item name: (type_identifier) @name)"),
        ("trait",  "(trait_item name: (type_identifier) @name)"),
        ("type",   "(type_item name: (type_identifier) @name)"),
        ("const",  "(const_item name: (identifier) @name)"),
        ("static", "(static_item name: (identifier) @name)"),
        ("mod",    "(mod_item name: (identifier) @name)"),
        ("macro",  "(macro_definition name: (identifier) @name)"),
    ],
};

static PYTHON: LangSpec = LangSpec {
    lang: tree_sitter_python::language,
    queries: &[
        ("def",   "(function_definition name: (identifier) @name)"),
        ("class", "(class_definition name: (identifier) @name)"),
    ],
};

static JAVASCRIPT: LangSpec = LangSpec {
    lang: tree_sitter_javascript::language,
    queries: &[
        ("function", "(function_declaration name: (identifier) @name)"),
        ("class",    "(class_declaration name: (identifier) @name)"),
        ("method",   "(method_definition name: (property_identifier) @name)"),
        ("arrow",    "(lexical_declaration (variable_declarator name: (identifier) @name value: (arrow_function)))"),
        ("arrow",    "(lexical_declaration (variable_declarator name: (identifier) @name value: (function_expression)))"),
    ],
};

static TYPESCRIPT: LangSpec = LangSpec {
    lang: tree_sitter_typescript::language_typescript,
    queries: &[
        ("function",  "(function_declaration name: (identifier) @name)"),
        ("class",     "(class_declaration name: (type_identifier) @name)"),
        ("method",    "(method_definition name: (property_identifier) @name)"),
        ("arrow",     "(lexical_declaration (variable_declarator name: (identifier) @name value: (arrow_function)))"),
        ("interface", "(interface_declaration name: (type_identifier) @name)"),
        ("type",      "(type_alias_declaration name: (type_identifier) @name)"),
        ("enum",      "(enum_declaration name: (identifier) @name)"),
    ],
};

static GO: LangSpec = LangSpec {
    lang: tree_sitter_go::language,
    queries: &[
        ("func",   "(function_declaration name: (identifier) @name)"),
        ("method", "(method_declaration name: (field_identifier) @name)"),
        ("type",   "(type_declaration (type_spec name: (type_identifier) @name))"),
    ],
};
