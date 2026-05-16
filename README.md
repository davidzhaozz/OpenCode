# OpenCode

A small, local-first coding agent written in Rust. It runs against any OpenAI-compatible chat endpoint — defaulting to [Ollama](https://ollama.com) with `llama3:8b`, but trivially repointed at `llama.cpp`'s `llama-server`, a stronger local model, or a hosted API.

The whole binary is ~4 MB with no Python or Node runtime to manage.

> **Note:** Not affiliated with the unrelated `sst/opencode` project.

## Commands

| Command | What it does |
| --- | --- |
| `opencode chat` | **Interactive REPL** with a tool-use loop, like Claude Code on the CLI. The model decides when to read files, grep, edit, write, or run shell commands. See [Chat mode](#chat-mode) below. |
| `opencode ask "..."` | One-shot question-answer over the current repo. Retrieves relevant chunks with a code-aware BM25 index (camelCase / snake_case split, `.gitignore`-aware), feeds them to the model. |
| `opencode edit <file> "..."` | Proposes a change to a file. Shows a colored unified diff. Asks `y/N` before writing. |
| `opencode scaffold --out <dir> "..."` | Generates a multi-file project from a spec. Previews the tree. Asks `y/N` before writing. |
| `opencode debug -- <cmd>` | Runs your command (`cargo test`, `pytest`, `make`, anything). On failure, feeds stderr + referenced files to the model, applies a proposed fix, retries. Max 3 iterations by default. |
| `opencode init` | Writes a default config to `~/.config/opencode/config.toml` (or the platform equivalent). |

All writes are gated on a `y/N` prompt unless you pass `--yes`.

## Chat mode

`opencode chat` starts an interactive REPL where the model investigates and edits your codebase on its own, using a small set of tools:

| Tool | What the model can do | Confirmation |
| --- | --- | --- |
| `find_symbol` | **Tree-sitter symbol lookup** by exact or partial name. Instant. | auto |
| `semantic_search` | **Hybrid BM25 + embedding search** for conceptual queries. | auto |
| `read_file` | Read any file with optional line range. Mtime-cached. | auto |
| `list_dir` | List directory entries. | auto |
| `grep` | Regex search (honors `.gitignore`). | auto |
| `edit_file` | Replace an exact substring in a file. | y/N |
| `write_file` | Create or overwrite a file. | y/N |
| `bash` | Run a shell command in the repo dir. | y/N |

When you start `chat`, OpenCode builds a small **intelligence layer** before the first prompt:

- A **repo manifest** (depth-capped tree + per-file one-line summaries) is injected into the system prompt so the model boots already oriented and skips the "let me explore" phase.
- A **tree-sitter symbol index** (Rust, Python, JS/TS, Go) makes `find_symbol("UserService")` return the exact definition line in milliseconds — replacing 3–5 grep+read calls per "where is X" question.
- A **persistent sqlite cache** at `~/.cache/opencode/cache.sqlite` stores file reads, symbol tables, file summaries, and dense embeddings. Second-run indexing is typically 20–30× faster than the first run (e.g. 5.8s → 0.2s on this repo).
- **Hybrid retrieval** (BM25 + dense embeddings, fused via Reciprocal Rank Fusion) handles both keyword and conceptual queries. Embeddings default to `nomic-embed-text` via Ollama — if the model isn't pulled, retrieval silently falls back to BM25-only.

```sh
# One-time, to enable embeddings:
ollama pull nomic-embed-text
```

The loop runs up to 25 tool calls per user turn before yielding back to you. `/reset` clears history, `/exit` or Ctrl-D leaves.

```sh
opencode chat
› where is BM25 scoring implemented?
→ Grep "BM25" in .
→ Read src/rag/bm25.rs
BM25 scoring lives in src/rag/bm25.rs:53 (the `score` method on `Bm25Index`). It uses the Okapi BM25 formula with k1=1.5, b=0.75, and a +1-smoothed IDF.

› add a doc comment above `score` explaining the formula
→ Read src/rag/bm25.rs
→ Edit src/rag/bm25.rs
[diff preview, y/N]
```

### Model requirements for chat

Tool calling requires a model that's actually trained for it.

- **`llama3:8b`** does NOT support tool calling — `chat` auto-upgrades it to `llama3.1:8b`.
- **`llama3.1:8b`** is the *minimum* viable model. Use it only for simple, one-shot tasks: a single grep, one targeted edit, a quick file question. It will misbehave on anything involving multiple coordinated edits, debugging loops, or careful refactors — expect hallucinated paths, malformed JSON arguments, and repeated identical tool calls.
- **For anything beyond trivial single edits, do NOT use `llama3.1:8b`.** Switch to **`qwen2.5-coder:14b`** (recommended floor) or **`qwen2.5-coder:32b`** if you have the RAM. These are coding-tuned and dramatically more reliable in the tool-use loop.

```sh
# One-time install
ollama pull qwen2.5-coder:14b

# Use it for chat without changing your config
opencode --model qwen2.5-coder:14b chat

# Or make it the default
echo 'model = "qwen2.5-coder:14b"' >> ~/.config/opencode/config.toml
```

**Rule of thumb:** if you find yourself reaching for `--yes` because the model is making the same mistake repeatedly, the model is too small. Upgrade before adding pressure.

Pass `--yes` to auto-confirm every write/edit/bash call — useful for trusted scripted runs, dangerous interactively.

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

OpenCode speaks the OpenAI chat-completions protocol, so **any compatible endpoint — local OR remote — works the same way**. There's no separate "remote model" mode; just point `--base-url` at the server.

| Backend | `base_url` | Notes |
| --- | --- | --- |
| Ollama (local, default) | `http://localhost:11434/v1` | `brew install ollama && brew services start ollama` |
| Ollama (remote) | `http://other-host:11434/v1` | Same Ollama, different machine. See [Remote local models](#remote-local-models) below. |
| llama.cpp | `http://localhost:8080/v1` | `llama-server -m model.gguf -c 8192 --host 127.0.0.1 --port 8080` |
| LM Studio | `http://localhost:1234/v1` | LM Studio's built-in server. |
| vLLM / TGI | `http://localhost:8000/v1` | Whatever your server exposes. |
| Together AI | `https://api.together.xyz/v1` | Hosted. Set `api_key`. |
| Groq | `https://api.groq.com/openai/v1` | Hosted. Set `api_key`. |
| Fireworks | `https://api.fireworks.ai/inference/v1` | Hosted. Set `api_key`. |
| OpenAI | `https://api.openai.com/v1` | Hosted. Set `api_key`. |

### Hosted-provider one-liners

Each block below is a single complete command — copy, set your key, run:

```sh
# Together AI — Qwen 2.5 Coder 32B (the cheapest "good" hosted coder)
OPENCODE_BASE_URL=https://api.together.xyz/v1 \
OPENCODE_API_KEY=$TOGETHER_API_KEY \
OPENCODE_MODEL=Qwen/Qwen2.5-Coder-32B-Instruct \
  opencode chat

# Groq — Llama 3.3 70B (fast)
OPENCODE_BASE_URL=https://api.groq.com/openai/v1 \
OPENCODE_API_KEY=$GROQ_API_KEY \
OPENCODE_MODEL=llama-3.3-70b-versatile \
  opencode chat

# Fireworks — DeepSeek V3
OPENCODE_BASE_URL=https://api.fireworks.ai/inference/v1 \
OPENCODE_API_KEY=$FIREWORKS_API_KEY \
OPENCODE_MODEL=accounts/fireworks/models/deepseek-v3 \
  opencode chat

# OpenAI — GPT-4o
OPENCODE_BASE_URL=https://api.openai.com/v1 \
OPENCODE_API_KEY=$OPENAI_API_KEY \
OPENCODE_MODEL=gpt-4o \
  opencode chat
```

Set the env vars once in your shell (`~/.zshrc`) and you can omit them from individual commands.

### Remote local models

To run OpenCode on your laptop but use a model running on a beefier desktop or server:

1. **On the server**, expose Ollama on the network:
   ```sh
   # bind to all interfaces instead of localhost
   OLLAMA_HOST=0.0.0.0:11434 ollama serve
   ```
   …or set the same env var in `brew services` / systemd.

2. **On the client**, point at it:
   ```sh
   opencode --base-url http://server.local:11434/v1 --model qwen2.5-coder:14b chat
   ```

For a private path, run it over **Tailscale** or an **SSH tunnel** (`ssh -L 11434:localhost:11434 server` then point at `http://localhost:11434/v1`). Don't expose Ollama directly on the public internet — it has no built-in auth.

## Recommended models

OpenCode is model-agnostic; the right pick depends on what you're asking it to do.

| Model | Pull command | Size | Best for | Notes |
| --- | --- | --- | --- | --- |
| **Llama 3 8B** | `ollama pull llama3:8b` | ~4.7 GB | `ask` only | The default for `ask`. **No tool calling** — `chat` will auto-upgrade away from it. |
| **Llama 3.1 8B** | `ollama pull llama3.1:8b` | ~4.9 GB | `ask`, trivial `chat` only | Minimum viable for `chat`. **Do not use for complicated edits, multi-step debugging, or autonomous loops** — it will hallucinate paths and spiral. |
| **Qwen 2.5 Coder 7B** | `ollama pull qwen2.5-coder:7b` | ~4.7 GB | `edit`, simple `chat` tasks | Coding-tuned 7B; better tool calling than Llama 3.1 8B at similar size. |
| **Qwen 2.5 Coder 14B** | `ollama pull qwen2.5-coder:14b` | ~9 GB | **`chat`, `scaffold`, real coding work** | **Recommended floor for `chat`.** Sweet spot on a 32 GB Mac. Use this if `llama3.1:8b` is misbehaving. |
| **Qwen 2.5 Coder 32B** | `ollama pull qwen2.5-coder:32b` | ~19 GB | Hard `debug` loops, larger codebases | Needs 32+ GB RAM. Closest local model to hosted-class for coding. |
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
