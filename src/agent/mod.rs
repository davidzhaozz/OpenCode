pub mod ask;
pub mod chat;
pub mod debug;
pub mod edit;
pub mod scaffold;

use crate::config::Config;

/// Shared system-prompt preamble. Language hint helps small models stay idiomatic.
pub fn system_preamble(cfg: &Config) -> String {
    let lang = cfg.language.as_deref().unwrap_or("the language used in the provided code");
    format!(
        "You are OpenCode, a careful local coding agent. \
         You read provided context and respond concisely. \
         Prefer correctness and idiomatic style for {lang}. \
         When asked to modify code, output exactly the requested format \
         (full file or unified diff) — no prose, no markdown fences \
         unless explicitly requested."
    )
}
