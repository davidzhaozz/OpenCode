use anyhow::{anyhow, Result};
use serde_json::Value;
use tokio::process::Command;
use tokio::time::{timeout, Duration};

use crate::diff;
use crate::exec;

use super::ToolCtx;

const DEFAULT_TIMEOUT_SECS: u64 = 60;
const MAX_TIMEOUT_SECS: u64 = 600;

pub async fn run(args: &Value, ctx: &ToolCtx) -> Result<String> {
    let cmd = args.get("cmd").and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("bash: missing 'cmd'"))?;
    let timeout_secs = args
        .get("timeout_secs")
        .and_then(|v| v.as_u64())
        .unwrap_or(DEFAULT_TIMEOUT_SECS)
        .min(MAX_TIMEOUT_SECS);

    println!("$ {cmd}");
    let apply = ctx.auto_yes || diff::confirm(&format!("Run this command?"))?;
    if !apply {
        return Ok(format!("user rejected command: {cmd}"));
    }

    let fut = Command::new("bash").arg("-c").arg(cmd).current_dir(&ctx.repo).output();
    let result = timeout(Duration::from_secs(timeout_secs), fut).await;
    let out = match result {
        Ok(Ok(o)) => o,
        Ok(Err(e)) => return Err(anyhow!("bash: spawn failed: {e}")),
        Err(_) => return Ok(format!("ERROR: command timed out after {timeout_secs}s")),
    };

    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    let exit = out.status.code().unwrap_or(-1);
    Ok(format!(
        "exit: {exit}\n--- stdout ---\n{}\n--- stderr ---\n{}",
        exec::tail(&stdout, 4000),
        exec::tail(&stderr, 4000),
    ))
}
