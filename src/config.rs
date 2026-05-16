use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub backend: Backend,
    pub model: String,
    /// Optional language hint passed to the model ("rust", "python", etc.)
    pub language: Option<String>,
    /// Tokens of context the backend can handle (used to budget retrieval)
    pub context_window: usize,
    /// Sampling temperature
    pub temperature: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Backend {
    /// Base URL of an OpenAI-compatible server.
    /// - llama.cpp llama-server:  http://localhost:8080/v1
    /// - Ollama:                  http://localhost:11434/v1
    /// - Together/Groq/etc:       https://api.together.xyz/v1
    pub base_url: String,
    /// Optional API key (sent as Bearer). Most local servers don't need it.
    pub api_key: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            backend: Backend {
                base_url: "http://localhost:11434/v1".to_string(),
                api_key: None,
            },
            model: "llama3:8b".to_string(),
            language: None,
            context_window: 8192,
            temperature: 0.2,
        }
    }
}

impl Config {
    pub fn load(override_path: Option<&Path>) -> Result<Self> {
        let path = override_path
            .map(PathBuf::from)
            .or_else(default_path)
            .filter(|p| p.exists());

        match path {
            Some(p) => {
                let text = std::fs::read_to_string(&p)
                    .with_context(|| format!("reading config at {}", p.display()))?;
                let cfg: Config = toml::from_str(&text)
                    .with_context(|| format!("parsing config at {}", p.display()))?;
                Ok(cfg)
            }
            None => Ok(Config::default()),
        }
    }

    pub fn write_default() -> Result<()> {
        let path = default_path().context("no config dir available")?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = toml::to_string_pretty(&Config::default())?;
        std::fs::write(&path, text)?;
        println!("wrote default config to {}", path.display());
        Ok(())
    }
}

fn default_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("opencode").join("config.toml"))
}
