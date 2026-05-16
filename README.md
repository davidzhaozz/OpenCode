# OpenCode

A small, local-first coding agent written in Rust. It runs against any OpenAI-compatible chat endpoint — defaulting to [Ollama](https://ollama.com) with `llama3:8b`, but trivially repointed at `llama.cpp`'s `llama-server`, a stronger local model, or a hosted API.

The whole binary is ~4 MB with no Python or Node runtime to manage.

> **Note:** Not affiliated with the unrelated `sst/opencode` project.

## Commands

| Command | What it does |
| --- | --- |
| `opencode ask "..."` | Question-answer over the current repo. Retrieves relevant chunks with a code-aware BM25 index (camelCase / snake_case split, `.gitignore`-aware), feeds them to the model. |
| `opencode edit <file> "..."` | Proposes a change to a file. Shows a colored unified diff. Asks `y/N` before writing. |
| `opencode scaffold --out <dir> "..."` | Generates a multi-file project from a spec. Previews the tree. Asks `y/N` before writing. |
| `opencode debug -- <cmd>` | Runs your command (`cargo test`, `pytest`, `make`, anything). On failure, feeds stderr + referenced files to the model, applies a proposed fix, retries. Max 3 iterations by default. |
| `opencode init` | Writes a default config to `~/.config/opencode/config.toml` (or the platform equivalent). |

All writes are gated on a `y/N` prompt unless you pass `--yes`.

## Install

### Prerequisites

- **Rust** (1.75+):
  ```sh
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
  ```
- **An LLM backend.** Easiest is Ollama:
  ```sh
  brew install ollama
  brew services start ollama
  ollama pull llama3:8b
  ```

### Build

```sh
git clone https://github.com/davidzhaozz/OpenCode.git
cd OpenCode
cargo build --release
# optional: put it on PATH
ln -s "$(pwd)/target/release/opencode" /opt/homebrew/bin/opencode
```

### First run

```sh
opencode init   # writes default config
cd ~/some-project
opencode ask "where is auth handled?"
```

## Configuration

`opencode init` writes a TOML config you can edit:

```toml
[backend]
base_url = "http://localhost:11434/v1"   # Ollama default
# api_key = "sk-..."                     # only if your backend needs one

model = "llama3:8b"
language = "rust"                        # optional hint passed to the model
context_window = 8192
temperature = 0.2
```

Pass `--config <path>` to use a different config file per invocation.

### Per-invocation overrides

Any config value can be overridden on the command line or via an environment variable, without touching the config file:

| Flag | Env var | Overrides |
| --- | --- | --- |
| `--model <name>` | `OPENCODE_MODEL` | `model` |
| `--base-url <url>` | `OPENCODE_BASE_URL` | `backend.base_url` |
| `--api-key <key>` | `OPENCODE_API_KEY` | `backend.api_key` |
| `--temperature <f>` | `OPENCODE_TEMPERATURE` | `temperature` |
| `--language <lang>` | `OPENCODE_LANGUAGE` | `language` |
| `--context-window <n>` | `OPENCODE_CONTEXT_WINDOW` | `context_window` |

Precedence: **CLI flag > env var > config file > built-in default.**

Examples:

```sh
# Try a stronger model for one debug run without touching config
opencode --model qwen2.5-coder:14b debug -- cargo test

# Point at a hosted endpoint via env vars
OPENCODE_BASE_URL=https://api.together.xyz/v1 \
OPENCODE_API_KEY=$TOGETHER_KEY \
OPENCODE_MODEL=Qwen/Qwen2.5-Coder-32B-Instruct \
  opencode ask "where is the retry logic?"

# One-off temperature bump for creative scaffolding
opencode --temperature 0.7 scaffold --out /tmp/idea "a tiny TUI todo app in rust"
```

### Swapping backends

OpenCode speaks the OpenAI chat-completions protocol, so any compatible endpoint works. Edit `base_url` and `model`:

| Backend | `base_url` | Notes |
| --- | --- | --- |
| Ollama (default) | `http://localhost:11434/v1` | `brew install ollama && brew services start ollama` |
| llama.cpp | `http://localhost:8080/v1` | `llama-server -m model.gguf -c 8192 --host 127.0.0.1 --port 8080` |
| Together AI | `https://api.together.xyz/v1` | Set `api_key`. |
| Groq | `https://api.groq.com/openai/v1` | Set `api_key`. |
| vLLM / TGI / LM Studio | `http://localhost:<port>/v1` | Whatever your server exposes. |

## Recommended models

OpenCode is model-agnostic; the right pick depends on what you're asking it to do.

| Model | Pull command | Size | Best for | Notes |
| --- | --- | --- | --- | --- |
| **Llama 3 8B** | `ollama pull llama3:8b` | ~4.7 GB | `ask`, simple `edit` | The default. Snappy on Apple Silicon. Will struggle with multi-file generation. |
| **Llama 3.1 8B** | `ollama pull llama3.1:8b` | ~4.9 GB | Same as above, slightly better reasoning | Drop-in upgrade. |
| **Qwen 2.5 Coder 7B** | `ollama pull qwen2.5-coder:7b` | ~4.7 GB | Edits, refactors | Coding-tuned 7B; punches above its weight on code tasks. |
| **Qwen 2.5 Coder 14B** | `ollama pull qwen2.5-coder:14b` | ~9 GB | `scaffold`, multi-file work | Sweet spot for autonomous flows on a 32 GB Mac. |
| **Qwen 2.5 Coder 32B** | `ollama pull qwen2.5-coder:32b` | ~19 GB | `debug` loops, larger codebases | Needs 32+ GB RAM. Closest local model to hosted-class for coding. |
| **DeepSeek-Coder-V2 16B** | `ollama pull deepseek-coder-v2:16b` | ~9 GB | Alt for `scaffold` / `debug` | MoE — fast inference for its size. |

After pulling, switch by editing `~/.config/opencode/config.toml`:

```toml
model = "qwen2.5-coder:14b"
```

Or override per-invocation by writing your own config file and pointing at it with `--config`.

### Practical guidance

- For `ask` and small targeted `edit`s, an **8B model** is fine.
- For `scaffold` (multi-file JSON output) or `debug` loops (multi-step reasoning), use **14B+**. 8B models will produce malformed JSON or chase their tails.
- For very large codebases, increase `context_window` to match your backend's limit, and bump `--k` on `ask` to retrieve more chunks.

## How it works

- **Retrieval.** `opencode ask` walks the repo respecting `.gitignore`, chunks files into ~60-line overlapping windows, and ranks them with BM25. The tokenizer splits on non-alphanumerics *and* camelCase / snake_case boundaries so a query for `parse json` matches `parseJsonResponse`.
- **LLM.** A single `LlmBackend` enum dispatches to an OpenAI-compatible HTTP client. Adding a new backend means adding a variant and one `chat(...)` impl.
- **Edits.** `edit` asks the model to return the full new file, strips stray markdown fences, prints a unified diff, and writes after confirmation. `scaffold` and `debug` ask for structured JSON (file list with full contents) and validate paths to prevent writes outside the target directory.
- **Debug loop.** `debug` greps stderr for referenced file paths, includes their contents as context, applies the model's proposed fix, and re-runs the command up to `--max-iters` times.

## Project layout

```
src/
├── main.rs           # entry
├── cli.rs            # clap subcommands
├── config.rs         # TOML config + default path
├── diff.rs           # colored unified diff + y/N prompt
├── exec.rs           # run user commands, capture stdout/stderr
├── llm/
│   ├── mod.rs        # LlmBackend enum, ChatMessage, GenOpts
│   └── openai.rs     # OpenAI-compatible HTTP client
├── rag/
│   ├── mod.rs        # Chunk, Retriever trait
│   ├── scan.rs       # walk repo, chunk files
│   └── bm25.rs       # BM25 + code-aware tokenizer
└── agent/
    ├── mod.rs        # shared system prompt
    ├── ask.rs
    ├── edit.rs
    ├── scaffold.rs
    └── debug.rs
```

## Limitations

- **No streaming.** Responses arrive whole. Fine for short answers; noticeable on long generations.
- **BM25, not vector RAG.** Great for symbol/identifier lookups, weaker on semantic queries. A `Retriever` trait makes swapping in embeddings straightforward.
- **No tool-use loop.** The agent doesn't iteratively `Read`/`Grep`/`Bash` like Claude Code or Aider — each command does one shot of retrieval + one model call (`debug` being the exception, which loops on the user's command).
- **Path safety, not sandbox.** `scaffold` and `debug` refuse absolute paths and `..` segments, but they trust your config and the model to write to the right repo. Run against repos you'd be comfortable letting an editor touch.

## License

MIT
