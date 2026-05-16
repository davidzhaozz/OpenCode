pub mod openai;

use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

impl ChatMessage {
    pub fn system(s: impl Into<String>) -> Self { Self { role: "system".into(), content: s.into() } }
    pub fn user(s: impl Into<String>) -> Self   { Self { role: "user".into(),   content: s.into() } }
}

/// Sampling knobs that callers can override per-request.
#[derive(Debug, Clone, Default)]
pub struct GenOpts {
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    /// JSON schema (or "json_object") to force structured output.
    pub json_mode: bool,
}

/// Trait for any chat-completions backend. We use an enum-dispatched concrete
/// type rather than `dyn Trait` so async fns work without `async_trait`.
pub enum LlmBackend {
    OpenAi(openai::OpenAiBackend),
}

impl LlmBackend {
    pub async fn chat(&self, messages: &[ChatMessage], opts: &GenOpts) -> Result<String> {
        match self {
            LlmBackend::OpenAi(b) => b.chat(messages, opts).await,
        }
    }
}
