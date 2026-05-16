use anyhow::{Context, Result};
use std::path::Path;
use tokio::process::Command;

pub struct RunOutput {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

/// Run a user-supplied command in `cwd`, capturing stdout+stderr.
pub async fn run(cwd: &Path, argv: &[String]) -> Result<RunOutput> {
    let (program, rest) = argv
        .split_first()
        .context("empty command")?;
    let output = Command::new(program)
        .args(rest)
        .current_dir(cwd)
        .output()
        .await
        .with_context(|| format!("running {program}"))?;
    Ok(RunOutput {
        status: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
    })
}

/// Truncate a stream from the END (most recent output) — that's where errors
/// usually live.
pub fn tail(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes { return s.to_string(); }
    let start = s.len() - max_bytes;
    let mut adjusted = start;
    while adjusted < s.len() && !s.is_char_boundary(adjusted) {
        adjusted += 1;
    }
    format!("... [{} bytes truncated]\n{}", adjusted, &s[adjusted..])
}
