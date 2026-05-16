use anyhow::Result;
use console::style;
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;
use serde_json::Value;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use crate::cache::Cache;
use crate::config::Config;
use crate::llm::{openai, AssistantMessage, ChatMessage, GenOpts, ToolCall};
use crate::manifest;
use crate::rag::{hybrid::HybridRetriever, scan::scan_repo};
use crate::symbols;
use crate::tools::{self, ToolCtx};

const MAX_TOOL_CALLS_PER_TURN: usize = 25;

pub async fn run(cfg: &Config, repo: &Path, auto_yes: bool) -> Result<()> {
    // ---- Build the intelligence layer up-front ----
    let cache = Arc::new(Cache::open()?);

    let t0 = Instant::now();
    eprint!("{} ", style("indexing").dim());
    let manifest = manifest::build(repo, &cache)?;
    eprint!("{} ", style(format!("manifest({} files)", manifest.file_count)).dim());

    let symbol_index = symbols::build(repo, &cache)?;
    let n_syms: usize = symbol_index.by_name.values().map(|v| v.len()).sum();
    eprint!("{} ", style(format!("symbols({n_syms})")).dim());

    let chunks = scan_repo(repo)?;
    let n_chunks = chunks.len();
    let retriever = crate::rag::hybrid::build(chunks, cfg, cache.clone()).await?;
    eprint!("{} ", style(format!("embeddings({n_chunks} chunks)")).dim());

    eprintln!("{}", style(format!("[{:.1}s]", t0.elapsed().as_secs_f32())).dim());

    let ctx = ToolCtx {
        repo: repo.to_path_buf(),
        auto_yes,
        cache,
        symbols: Arc::new(symbol_index),
        retriever: Arc::new(retriever),
    };

    let backend = openai::build(cfg);
    let tool_defs = tools::definitions();

    let mut history: Vec<ChatMessage> = vec![ChatMessage::system(system_prompt(repo, &manifest.rendered))];

    let mut rl = DefaultEditor::new()?;
    eprintln!(
        "{} {} — model: {}, repo: {}",
        style("OpenCode chat").bold().cyan(),
        style(env!("CARGO_PKG_VERSION")).dim(),
        style(&cfg.model).yellow(),
        style(repo.display().to_string()).dim()
    );
    eprintln!("Type a message. Ctrl-D, /exit, or /quit to leave. /reset clears history.\n");

    loop {
        let line = match rl.readline(&style("› ").cyan().to_string()) {
            Ok(l) => l,
            Err(ReadlineError::Eof) | Err(ReadlineError::Interrupted) => {
                eprintln!();
                break;
            }
            Err(e) => return Err(e.into()),
        };
        let input = line.trim();
        if input.is_empty() { continue; }
        if input == "/exit" || input == "/quit" { break; }
        if input == "/reset" {
            history.truncate(1);
            eprintln!("{}", style("(conversation reset)").dim());
            continue;
        }
        let _ = rl.add_history_entry(input);

        history.push(ChatMessage::user(input));
        if let Err(e) = run_turn(&backend, &tool_defs, &ctx, &mut history).await {
            eprintln!("{} {e}", style("error:").red().bold());
        }
        eprintln!();
    }
    Ok(())
}

async fn run_turn(
    backend: &crate::llm::LlmBackend,
    tool_defs: &[Value],
    ctx: &ToolCtx,
    history: &mut Vec<ChatMessage>,
) -> Result<()> {
    for iter in 0..MAX_TOOL_CALLS_PER_TURN {
        let opts = GenOpts {
            tools: tool_defs.to_vec(),
            ..Default::default()
        };
        let msg: AssistantMessage = backend.chat(history, &opts).await?;

        if msg.tool_calls.is_empty() {
            history.push(ChatMessage::assistant_with_calls(msg.content.clone(), vec![]));
            if msg.content.is_empty() {
                eprintln!("{}", style("(empty response)").dim());
            } else {
                println!("{}", msg.content);
            }
            return Ok(());
        }

        history.push(ChatMessage::assistant_with_calls(msg.content.clone(), msg.tool_calls.clone()));
        if !msg.content.is_empty() {
            println!("{}", msg.content);
        }

        for call in &msg.tool_calls {
            let label = pretty_call_label(call);
            eprintln!("{} {label}", style("→").cyan().bold());
            let result = match tools::dispatch(&call.function.name, &call.function.arguments, ctx).await {
                Ok(s) => s,
                Err(e) => format!("ERROR: {e}"),
            };
            let head = preview(&result);
            eprintln!("{}", style(format!("  {} ({} chars)", head, result.len())).dim());
            history.push(ChatMessage::tool_result(&call.id, &call.function.name, result));
        }

        if iter + 1 == MAX_TOOL_CALLS_PER_TURN {
            eprintln!(
                "{}",
                style(format!(
                    "(hit tool-call cap of {MAX_TOOL_CALLS_PER_TURN}; ending turn — ask again to continue)"
                ))
                .yellow()
            );
            return Ok(());
        }
    }
    Ok(())
}

fn system_prompt(repo: &Path, manifest_rendered: &str) -> String {
    format!(
        "You are OpenCode, an interactive coding agent operating in the user's repository.\n\
         \n\
         You have tools to investigate and modify code:\n\
         - find_symbol: look up a function/struct/class/etc. by name. **Use this FIRST** when you want \
           to know where something is defined — it's faster and more accurate than grep.\n\
         - semantic_search: hybrid BM25 + embedding search for conceptual queries (\"retry logic\", \
           \"auth flow\"). Use this when you don't know the exact name to look for.\n\
         - grep: regex search across the repo. Use for strings, comments, anything not a defined symbol.\n\
         - read_file: read a file's contents. ALWAYS use this before editing a file. Results are cached.\n\
         - list_dir: list a directory's entries.\n\
         - edit_file: replace an exact substring in a file (user confirms).\n\
         - write_file: create or overwrite a file (user confirms).\n\
         - bash: run a shell command (user confirms). Use for build/test/format/git/ls.\n\
         \n\
         How to work:\n\
         - Use find_symbol FIRST when looking for a known name; fall back to grep / semantic_search.\n\
         - Read a file BEFORE editing it — edit_file requires an exact unique substring match.\n\
         - Prefer edit_file over write_file when modifying existing files.\n\
         - After making changes, suggest or run a verification command (cargo check, pytest, etc.).\n\
         - When the task is complete, reply with a SHORT plain-text summary and emit NO further tool calls.\n\
         - If a tool returns ERROR, read the message carefully and adjust — don't repeat the same call.\n\
         \n\
         Working directory: {}\n\
         \n\
         === Repository overview ===\n\
         {}\n\
         === End repository overview ===\n",
        repo.display(),
        manifest_rendered,
    )
}

fn pretty_call_label(call: &ToolCall) -> String {
    let args: Value = serde_json::from_str(&call.function.arguments).unwrap_or(Value::Null);
    let s = |k: &str| args.get(k).and_then(|v| v.as_str()).unwrap_or("?");
    match call.function.name.as_str() {
        "read_file"        => format!("{} {}", style("Read").green(), s("path")),
        "find_symbol"      => format!("{} {}", style("FindSymbol").green(), s("name")),
        "semantic_search"  => format!("{} {:?}", style("SemSearch").green(), s("query")),
        "list_dir"         => format!("{} {}", style("List").green(), args.get("path").and_then(|v| v.as_str()).unwrap_or(".")),
        "grep"             => format!("{} {:?} in {}", style("Grep").green(), s("pattern"), args.get("path").and_then(|v| v.as_str()).unwrap_or(".")),
        "edit_file"        => format!("{} {}", style("Edit").yellow(), s("path")),
        "write_file"       => format!("{} {}", style("Write").yellow(), s("path")),
        "bash"             => format!("{} {}", style("Bash").magenta(), s("cmd")),
        other              => format!("{other}({})", call.function.arguments),
    }
}

/// First non-empty line, truncated to 80 chars.
fn preview(s: &str) -> String {
    let line = s.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim();
    if line.chars().count() <= 80 {
        line.to_string()
    } else {
        let cut: String = line.chars().take(77).collect();
        format!("{cut}...")
    }
}
