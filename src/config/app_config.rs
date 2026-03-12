use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

/// Application configuration stored in config directory
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// API key (legacy)
    #[serde(skip_serializing_if = "Option::is_none")]
    api_key: Option<String>,

    /// API URL
    #[serde(default = "default_api_url")]
    api_url: String,

    /// Auth URL
    #[serde(default = "default_auth_url")]
    auth_url: String,

    /// Relay URL
    #[serde(default = "default_relay_url")]
    relay_url: String,

    /// OAuth access token
    #[serde(skip_serializing_if = "Option::is_none")]
    access_token: Option<String>,

    /// OAuth refresh token
    #[serde(skip_serializing_if = "Option::is_none")]
    refresh_token: Option<String>,

    /// Token expiration timestamp (milliseconds)
    #[serde(skip_serializing_if = "Option::is_none")]
    token_expires_at: Option<u64>,

    /// Agent token (for relay connection)
    #[serde(skip_serializing_if = "Option::is_none")]
    agent_token: Option<String>,

    /// Machine ID
    #[serde(skip_serializing_if = "Option::is_none")]
    machine_id: Option<String>,

    /// First run welcome flag
    #[serde(default)]
    has_seen_welcome: bool,
}

fn default_api_url() -> String {
    "https://api.sessioncast.io".to_string()
}

fn default_auth_url() -> String {
    "https://auth.sessioncast.io".to_string()
}

fn default_relay_url() -> String {
    "wss://relay.sessioncast.io/ws".to_string()
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            api_key: None,
            api_url: default_api_url(),
            auth_url: default_auth_url(),
            relay_url: default_relay_url(),
            access_token: None,
            refresh_token: None,
            token_expires_at: None,
            agent_token: None,
            machine_id: None,
            has_seen_welcome: false,
        }
    }
}

impl AppConfig {
    /// Load configuration from disk
    pub fn load() -> anyhow::Result<Self> {
        confy::load("sessioncast", "config").map_err(Into::into)
    }

    /// Save configuration to disk
    pub fn save(&self) -> anyhow::Result<()> {
        confy::store("sessioncast", "config", self).map_err(Into::into)
    }

    // Getters
    pub fn api_key(&self) -> Option<&str> {
        self.api_key.as_deref()
    }

    pub fn api_url(&self) -> &str {
        &self.api_url
    }

    pub fn auth_url(&self) -> &str {
        &self.auth_url
    }

    pub fn relay_url(&self) -> &str {
        &self.relay_url
    }

    pub fn access_token(&self) -> Option<&str> {
        // Check if token is expired
        if let (Some(token), Some(expires_at)) = (&self.access_token, self.token_expires_at) {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);

            if now < expires_at {
                return Some(token);
            }
        }
        None
    }

    pub fn refresh_token(&self) -> Option<&str> {
        self.refresh_token.as_deref()
    }

    pub fn agent_token(&self) -> Option<&str> {
        self.agent_token.as_deref()
    }

    pub fn machine_id(&self) -> Option<&str> {
        self.machine_id.as_deref()
    }

    pub fn is_logged_in(&self) -> bool {
        self.api_key.is_some() || self.access_token().is_some() || self.agent_token.is_some()
    }

    pub fn has_seen_welcome(&self) -> bool {
        self.has_seen_welcome
    }

    // Setters
    pub fn set_api_key(&mut self, key: String) {
        self.api_key = Some(key);
    }

    pub fn set_api_url(&mut self, url: String) {
        self.api_url = url;
    }

    pub fn set_auth_url(&mut self, url: String) {
        self.auth_url = url;
    }

    pub fn set_relay_url(&mut self, url: String) {
        self.relay_url = url;
    }

    pub fn set_access_token(&mut self, token: String, expires_in: u64) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        self.access_token = Some(token);
        self.token_expires_at = Some(now + expires_in * 1000);
    }

    pub fn set_refresh_token(&mut self, token: String) {
        self.refresh_token = Some(token);
    }

    pub fn set_agent_token(&mut self, token: String) {
        self.agent_token = Some(token);
    }

    pub fn set_machine_id(&mut self, id: String) {
        self.machine_id = Some(id);
    }

    pub fn set_seen_welcome(&mut self) {
        self.has_seen_welcome = true;
    }

    /// Clear all authentication data
    pub fn clear_auth(&mut self) {
        self.api_key = None;
        self.access_token = None;
        self.refresh_token = None;
        self.token_expires_at = None;
        self.agent_token = None;
    }
}
