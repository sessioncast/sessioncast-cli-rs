mod app_config;

pub use app_config::*;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Agent configuration (from YAML file or OAuth)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentConfig {
    pub machine_id: String,
    pub relay: String,
    pub token: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enc_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api: Option<ApiConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ApiConfig {
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exec: Option<ExecConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub llm: Option<LlmConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<CapabilitiesConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecConfig {
    pub enabled: bool,
    pub shell: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub working_dir: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_commands: Option<Vec<String>>,
    pub default_timeout: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmConfig {
    pub enabled: bool,
    pub provider: String,
    #[serde(default)]
    pub base_url: String,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct CapabilitiesConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exec: Option<CapabilitySetting>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exec_cwd: Option<CapabilitySetting>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub llm_chat: Option<CapabilitySetting>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub send_keys: Option<CapabilitySetting>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub list_sessions: Option<CapabilitySetting>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CapabilitySetting {
    Bool(bool),
    Ask(String),
}

impl CapabilitySetting {
    pub fn is_granted(&self) -> bool {
        match self {
            CapabilitySetting::Bool(b) => *b,
            CapabilitySetting::Ask(s) => s == "ask",
        }
    }
}

impl AgentConfig {
    /// Load configuration from file or create default from OAuth token
    pub async fn load(config_path: Option<&str>) -> anyhow::Result<Self> {
        let app_config = crate::config::AppConfig::load()?;

        // Check for agent token (OAuth flow)
        if let Some(token) = app_config.agent_token() {
            let machine_id = get_hostname();

            // Try to fetch relay URL from Platform API
            let relay_url = Self::fetch_relay_url(token)
                .await
                .unwrap_or_else(|| app_config.relay_url().to_string());

            return Ok(Self {
                machine_id,
                relay: relay_url,
                token: token.to_string(),
                enc_key: None,
                api: Some(ApiConfig::default()),
            });
        }

        // Load from file
        let config_path = config_path
            .map(PathBuf::from)
            .or_else(|| std::env::var("SESSIONCAST_CONFIG").ok().map(PathBuf::from))
            .or_else(|| std::env::var("TMUX_REMOTE_CONFIG").ok().map(PathBuf::from))
            .or_else(|| {
                directories::BaseDirs::new().map(|dirs| dirs.home_dir().join(".sessioncast.yml"))
            });

        let path = config_path.ok_or_else(|| anyhow::anyhow!("Config file not found"))?;

        if !path.exists() {
            anyhow::bail!("Config file not found: {}", path.display());
        }

        let content = std::fs::read_to_string(&path)?;
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");

        if ext == "json" {
            Ok(serde_json::from_str(&content)?)
        } else {
            Ok(serde_yaml::from_str(&content)?)
        }
    }

    async fn fetch_relay_url(token: &str) -> Option<String> {
        let app_config = crate::config::AppConfig::load().ok()?;
        let url = format!(
            "{}/public/agent-tokens/{}/relay",
            app_config.api_url(),
            token
        );

        let client = reqwest::Client::new();
        let response = client
            .get(&url)
            .timeout(std::time::Duration::from_secs(3))
            .send()
            .await
            .ok()?;

        if !response.status().is_success() {
            return None;
        }

        #[derive(serde::Deserialize)]
        struct RelayResponse {
            relay_url: String,
        }

        response
            .json::<RelayResponse>()
            .await
            .ok()
            .map(|r| r.relay_url)
    }
}

fn get_hostname() -> String {
    #[cfg(unix)]
    {
        std::process::Command::new("hostname")
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_else(|_| "unknown".to_string())
    }
    #[cfg(windows)]
    {
        std::process::Command::new("hostname")
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_else(|_| "unknown".to_string())
    }
}
