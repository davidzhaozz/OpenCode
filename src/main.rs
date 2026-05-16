mod agent;
mod cache;
mod cli;
mod config;
mod diff;
mod exec;
mod llm;
mod manifest;
mod rag;
mod symbols;
mod tools;

use anyhow::Result;
use clap::Parser;

#[tokio::main]
async fn main() -> Result<()> {
    let args = cli::Cli::parse();
    cli::run(args).await
}
