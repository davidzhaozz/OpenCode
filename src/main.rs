mod agent;
mod cli;
mod config;
mod diff;
mod exec;
mod llm;
mod rag;
mod tools;

use anyhow::Result;
use clap::Parser;

#[tokio::main]
async fn main() -> Result<()> {
    let args = cli::Cli::parse();
    cli::run(args).await
}
