//! Persistent cache at ~/.cache/opencode/cache.sqlite.
//!
//! Four caches, all auto-invalidated on file mtime where applicable:
//! - `file_reads`     — verbatim file contents
//! - `file_summaries` — one-line per-file summaries for the manifest
//! - `symbols`        — per-file extracted symbols (JSON blob)
//! - `embeddings`     — vector embeddings keyed by (model, content_hash)

use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::UNIX_EPOCH;

/// Thread-safe cache. The inner sqlite connection is wrapped in a Mutex so
/// `Cache: Sync` (required for `&Cache` to cross await points in tokio).
pub struct Cache {
    conn: Mutex<Connection>,
}

impl Cache {
    pub fn open() -> Result<Self> {
        let path = default_path().context("no cache dir available")?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(&path)
            .with_context(|| format!("opening cache at {}", path.display()))?;
        conn.execute_batch(SCHEMA).context("initializing cache schema")?;
        // Reasonable defaults: WAL for concurrent reads while we write.
        let _ = conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;");
        Ok(Self { conn: Mutex::new(conn) })
    }

    // ---- file_reads ----

    pub fn get_file_read(&self, path: &Path) -> Option<String> {
        let key = path.to_string_lossy();
        let current = mtime_secs(path)?;
        let row: rusqlite::Result<(i64, String)> = self.conn.lock().unwrap().query_row(
            "SELECT mtime_secs, contents FROM file_reads WHERE path = ?1",
            params![key],
            |r| Ok((r.get(0)?, r.get(1)?)),
        );
        match row {
            Ok((cached_mtime, contents)) if cached_mtime == current => Some(contents),
            _ => None,
        }
    }

    pub fn put_file_read(&self, path: &Path, contents: &str) -> Result<()> {
        let Some(mtime) = mtime_secs(path) else { return Ok(()); };
        let size = contents.len() as i64;
        self.conn.lock().unwrap().execute(
            "INSERT OR REPLACE INTO file_reads(path, mtime_secs, size_bytes, contents) \
             VALUES (?1, ?2, ?3, ?4)",
            params![path.to_string_lossy(), mtime, size, contents],
        )?;
        Ok(())
    }

    // ---- file_summaries ----

    pub fn get_summary(&self, path: &Path) -> Option<String> {
        let current = mtime_secs(path)?;
        let row: rusqlite::Result<(i64, String)> = self.conn.lock().unwrap().query_row(
            "SELECT mtime_secs, summary FROM file_summaries WHERE path = ?1",
            params![path.to_string_lossy()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        );
        match row {
            Ok((cached_mtime, summary)) if cached_mtime == current => Some(summary),
            _ => None,
        }
    }

    pub fn put_summary(&self, path: &Path, summary: &str) -> Result<()> {
        let Some(mtime) = mtime_secs(path) else { return Ok(()); };
        self.conn.lock().unwrap().execute(
            "INSERT OR REPLACE INTO file_summaries(path, mtime_secs, summary) VALUES (?1, ?2, ?3)",
            params![path.to_string_lossy(), mtime, summary],
        )?;
        Ok(())
    }

    // ---- symbols ----

    pub fn get_symbols(&self, path: &Path) -> Option<String> {
        let current = mtime_secs(path)?;
        let row: rusqlite::Result<(i64, String)> = self.conn.lock().unwrap().query_row(
            "SELECT mtime_secs, blob FROM symbols WHERE path = ?1",
            params![path.to_string_lossy()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        );
        match row {
            Ok((cached_mtime, blob)) if cached_mtime == current => Some(blob),
            _ => None,
        }
    }

    pub fn put_symbols(&self, path: &Path, blob: &str) -> Result<()> {
        let Some(mtime) = mtime_secs(path) else { return Ok(()); };
        self.conn.lock().unwrap().execute(
            "INSERT OR REPLACE INTO symbols(path, mtime_secs, blob) VALUES (?1, ?2, ?3)",
            params![path.to_string_lossy(), mtime, blob],
        )?;
        Ok(())
    }

    /// All cached symbol blobs across the repo (caller filters/dedup).
    pub fn all_symbols(&self) -> Result<Vec<(PathBuf, String)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT path, blob FROM symbols")?;
        let rows = stmt.query_map([], |r| {
            let p: String = r.get(0)?;
            let b: String = r.get(1)?;
            Ok((PathBuf::from(p), b))
        })?;
        let mut out = Vec::new();
        for row in rows { out.push(row?); }
        Ok(out)
    }

    // ---- embeddings ----

    pub fn get_embedding(&self, model: &str, content_hash: &str) -> Option<Vec<f32>> {
        let row: rusqlite::Result<Vec<u8>> = self.conn.lock().unwrap().query_row(
            "SELECT vec FROM embeddings WHERE model = ?1 AND content_hash = ?2",
            params![model, content_hash],
            |r| r.get(0),
        );
        row.ok().map(|bytes| bytes_to_f32_le(&bytes))
    }

    pub fn put_embedding(&self, model: &str, content_hash: &str, vec: &[f32]) -> Result<()> {
        let bytes = f32_to_bytes_le(vec);
        self.conn.lock().unwrap().execute(
            "INSERT OR REPLACE INTO embeddings(model, content_hash, vec) VALUES (?1, ?2, ?3)",
            params![model, content_hash, bytes],
        )?;
        Ok(())
    }
}

/// SHA-256 hex digest of arbitrary input. Used as the cache key for embeddings.
pub fn content_hash(s: &str) -> String {
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    format!("{:x}", h.finalize())
}

fn mtime_secs(path: &Path) -> Option<i64> {
    let meta = std::fs::metadata(path).ok()?;
    let mtime = meta.modified().ok()?;
    let dur = mtime.duration_since(UNIX_EPOCH).ok()?;
    Some(dur.as_secs() as i64)
}

fn default_path() -> Option<PathBuf> {
    dirs::cache_dir().map(|d| d.join("opencode").join("cache.sqlite"))
}

fn f32_to_bytes_le(v: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len() * 4);
    for f in v {
        out.extend_from_slice(&f.to_le_bytes());
    }
    out
}

fn bytes_to_f32_le(b: &[u8]) -> Vec<f32> {
    b.chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS file_reads (
    path TEXT PRIMARY KEY,
    mtime_secs INTEGER NOT NULL,
    size_bytes INTEGER NOT NULL,
    contents TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS file_summaries (
    path TEXT PRIMARY KEY,
    mtime_secs INTEGER NOT NULL,
    summary TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS symbols (
    path TEXT PRIMARY KEY,
    mtime_secs INTEGER NOT NULL,
    blob TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS embeddings (
    model TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    vec BLOB NOT NULL,
    PRIMARY KEY (model, content_hash)
);
"#;
