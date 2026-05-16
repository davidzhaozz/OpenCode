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

    /// Model name (overrides config). Env: OPENCODE_MODEL
    #[arg(long, global = true, env = "OPENCODE_MODEL")]
    pub model: Option<String>,

    /// Backend base URL (overrides config). Env: OPENCODE_BASE_URL
    #[arg(long, global = true, env = "OPENCODE_BASE_URL")]
    pub base_url: Option<String>,

    /// Backend API key (overrides config). Env: OPENCODE_API_KEY
    #[arg(long, global = true, env = "OPENCODE_API_KEY", hide_env_values = true)]
    pub api_key: Option<String>,

    /// Sampling temperature (overrides config). Env: OPENCODE_TEMPERATURE
    #[arg(long, global = true, env = "OPENCODE_TEMPERATURE")]
    pub temperature: Option<f32>,

    /// Language hint passed to the model. Env: OPENCODE_LANGUAGE
    #[arg(long, global = true, env = "OPENCODE_LANGUAGE")]
    pub language: Option<String>,

    /// Context window the backend supports. Env: OPENCODE_CONTEXT_WINDOW
    #[arg(long, global = true, env = "OPENCODE_CONTEXT_WINDOW")]
    pub context_window: Option<usize>,

    /// Embedding model (overrides config). Env: OPENCODE_EMBEDDING_MODEL
    #[arg(long, global = true, env = "OPENCODE_EMBEDDING_MODEL")]
    pub embedding_model: Option<String>,

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

    /// Interactive chat REPL with a tool-use loop (Claude-Code-style).
    /// Defaults to llama3.1:8b if no model is specified — it has reliable tool calling.
    Chat {
        /// Auto-confirm all edits, writes, and shell commands. Use with caution.
        #[arg(long)]
        yes: bool,
    },
}

pub async fn run(args: Cli) -> Result<()> {
    let mut cfg = Config::load(args.config.as_deref())?;
    if let Some(m) = args.model { cfg.model = m; }
    if let Some(u) = args.base_url { cfg.backend.base_url = u; }
    if let Some(k) = args.api_key { cfg.backend.api_key = Some(k); }
    if let Some(t) = args.temperature { cfg.temperature = t; }
    if let Some(l) = args.language { cfg.language = Some(l); }
    if let Some(c) = args.context_window { cfg.context_window = c; }
    if let Some(e) = args.embedding_model { cfg.embedding_model = e; }

    let repo = args
        .repo
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
        Cmd::Chat { yes } => {
            // Default to llama3.1:8b for chat — it has native tool calling.
            // llama3:8b (non-3.1) emits broken/fake tool calls.
            if cfg.model == "llama3:8b" {
                cfg.model = "llama3.1:8b".to_string();
                eprintln!("(chat: upgraded model llama3:8b → llama3.1:8b for tool calling; override with --model)");
            }
            agent::chat::run(&cfg, &repo, yes).await?
        }
    }
    Ok(())
}
