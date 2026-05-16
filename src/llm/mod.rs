pub mod embeddings;
pub mod openai;

use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub tool_call_id: Option<String>,
    /// Optional human-readable name for tool messages.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub name: Option<String>,
}

impl ChatMessage {
    pub fn system(s: impl Into<String>) -> Self {
        Self { role: "system".into(), content: Some(s.into()), tool_calls: None, tool_call_id: None, name: None }
    }
    pub fn user(s: impl Into<String>) -> Self {
        Self { role: "user".into(), content: Some(s.into()), tool_calls: None, tool_call_id: None, name: None }
    }
    pub fn assistant_with_calls(content: String, tool_calls: Vec<ToolCall>) -> Self {
        Self {
            role: "assistant".into(),
            content: if content.is_empty() { None } else { Some(content) },
            tool_calls: if tool_calls.is_empty() { None } else { Some(tool_calls) },
            tool_call_id: None,
            name: None,
        }
    }
    /// Construct a `tool` result message to feed back into the conversation.
    pub fn tool_result(call_id: impl Into<String>, name: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            role: "tool".into(),
            content: Some(body.into()),
            tool_calls: None,
            tool_call_id: Some(call_id.into()),
            name: Some(name.into()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type", default = "default_call_kind")]
    pub kind: String,
    pub function: ToolCallFunction,
}

fn default_call_kind() -> String { "function".into() }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallFunction {
    pub name: String,
    /// JSON-encoded arguments — kept as a string because that's how the OpenAI
    /// schema returns them and re-serializes them.
    pub arguments: String,
}

/// What the model returned for one assistant turn.
#[derive(Debug, Clone)]
pub struct AssistantMessage {
    pub content: String,
    pub tool_calls: Vec<ToolCall>,
}

/// Sampling knobs that callers can override per-request.
#[derive(Debug, Clone, Default)]
pub struct GenOpts {
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    /// JSON schema (or "json_object") to force structured output.
    pub json_mode: bool,
    /// Tool definitions passed through to the model.
    pub tools: Vec<serde_json::Value>,
}

pub enum LlmBackend {
    OpenAi(openai::OpenAiBackend),
}

impl LlmBackend {
    pub async fn chat(&self, messages: &[ChatMessage], opts: &GenOpts) -> Result<AssistantMessage> {
        match self {
            LlmBackend::OpenAi(b) => b.chat(messages, opts).await,
        }
    }
}
