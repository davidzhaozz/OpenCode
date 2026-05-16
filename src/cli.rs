use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

use crate::agent;
use crate::config::Config;

#[derive(Parser, Debug)]
#[command(name = "opencode", version, about = "OpenCode — local coding agent backed by any OpenAI-compatible LLM")]
pub struct Cli {
    /// Path to repo root (defaults to cwd)
    #[arg(long, global = true)]
    pub repo: Option<PathBuf>,

    /// Override config path
    #[arg(long, global = true)]
    pub config: Option<PathBuf>,

    #[command(subcommand)]
    pub cmd: Cmd,
}

#[derive(Subcommand, Debug)]
pub enum Cmd {
    /// Write a default config file to ~/.config/opencode/config.toml
    Init,

    /// Ask a question about the codebase (RAG over current repo)
    Ask {
        /// The question
        question: Vec<String>,
        /// Max chunks to retrieve
        #[arg(long, default_value_t = 8)]
        k: usize,
    },

    /// Propose an edit to a file and show a diff before applying
    Edit {
        /// File to edit
        file: PathBuf,
        /// What to do
        instruction: Vec<String>,
        /// Apply without confirmation
        #[arg(long)]
        yes: bool,
    },

    /// Scaffold a new project or file tree from a spec
    Scaffold {
        /// Target directory (must not exist or be empty)
        #[arg(long)]
        out: PathBuf,
        /// Description of what to build
        spec: Vec<String>,
        /// Write without confirmation
        #[arg(long)]
        yes: bool,
    },

    /// Run a command, capture errors, propose a fix as a diff, retry
    Debug {
        /// Max iterations
        #[arg(long, default_value_t = 3)]
        max_iters: usize,
        /// Apply fixes without confirmation
        #[arg(long)]
        yes: bool,
        /// Command to run after `--`
        #[arg(last = true, required = true)]
        cmd: Vec<String>,
    },
}

pub async fn run(args: Cli) -> Result<()> {
    let cfg = Config::load(args.config.as_deref())?;
    let repo = args
        .repo
        .clone()
        .unwrap_or_else(|| std::env::current_dir().expect("cwd"));

    match args.cmd {
        Cmd::Init => Config::write_default()?,
        Cmd::Ask { question, k } => {
            agent::ask::run(&cfg, &repo, &question.join(" "), k).await?
        }
        Cmd::Edit { file, instruction, yes } => {
            agent::edit::run(&cfg, &repo, &file, &instruction.join(" "), yes).await?
        }
        Cmd::Scaffold { out, spec, yes } => {
            agent::scaffold::run(&cfg, &out, &spec.join(" "), yes).await?
        }
        Cmd::Debug { max_iters, yes, cmd } => {
            agent::debug::run(&cfg, &repo, &cmd, max_iters, yes).await?
        }
    }
    Ok(())
}
