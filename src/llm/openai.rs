use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::llm::{ChatMessage, GenOpts, LlmBackend};

pub struct OpenAiBackend {
    base_url: String,
    api_key: Option<String>,
    model: String,
    default_temperature: f32,
    client: reqwest::Client,
}

impl OpenAiBackend {
    pub fn from_config(cfg: &Config) -> Self {
        Self {
            base_url: cfg.backend.base_url.trim_end_matches('/').to_string(),
            api_key: cfg.backend.api_key.clone(),
            model: cfg.model.clone(),
            default_temperature: cfg.temperature,
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(600))
                .build()
                .expect("reqwest client"),
        }
    }

    pub async fn chat(&self, messages: &[ChatMessage], opts: &GenOpts) -> Result<String> {
        #[derive(Serialize)]
        struct Req<'a> {
            model: &'a str,
            messages: &'a [ChatMessage],
            temperature: f32,
            #[serde(skip_serializing_if = "Option::is_none")]
            max_tokens: Option<u32>,
            #[serde(skip_serializing_if = "Option::is_none")]
            response_format: Option<ResponseFormat>,
            stream: bool,
        }
        #[derive(Serialize)]
        struct ResponseFormat { #[serde(rename = "type")] kind: &'static str }

        #[derive(Deserialize)]
        struct Resp { choices: Vec<Choice> }
        #[derive(Deserialize)]
        struct Choice { message: RespMsg }
        #[derive(Deserialize)]
        struct RespMsg { content: String }

        let url = format!("{}/chat/completions", self.base_url);
        let req = Req {
            model: &self.model,
            messages,
            temperature: opts.temperature.unwrap_or(self.default_temperature),
            max_tokens: opts.max_tokens,
            response_format: if opts.json_mode {
                Some(ResponseFormat { kind: "json_object" })
            } else { None },
            stream: false,
        };

        let mut builder = self.client.post(&url).json(&req);
        if let Some(k) = &self.api_key {
            builder = builder.bearer_auth(k);
        }
        let resp = builder.send().await.context("LLM request failed")?;
        let status = resp.status();
        let body = resp.text().await.context("reading LLM response")?;
        if !status.is_success() {
            return Err(anyhow!("LLM backend returned {}: {}", status, body));
        }
        let parsed: Resp = serde_json::from_str(&body)
            .with_context(|| format!("parsing LLM response: {}", body))?;
        let content = parsed
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("no choices in LLM response"))?
            .message
            .content;
        Ok(content)
    }
}

pub fn build(cfg: &Config) -> LlmBackend {
    LlmBackend::OpenAi(OpenAiBackend::from_config(cfg))
}
