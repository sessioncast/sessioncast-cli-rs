mod client;
mod message;

pub use client::*;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// WebSocket message types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    #[serde(rename = "type")]
    pub msg_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<HashMap<String, String>>,
}

impl Message {
    pub fn new(msg_type: &str) -> Self {
        Self {
            msg_type: msg_type.to_string(),
            role: None,
            session: None,
            payload: None,
            meta: None,
        }
    }

    pub fn role(mut self, role: &str) -> Self {
        self.role = Some(role.to_string());
        self
    }

    pub fn session(mut self, session: &str) -> Self {
        self.session = Some(session.to_string());
        self
    }

    pub fn payload(mut self, payload: &str) -> Self {
        self.payload = Some(payload.to_string());
        self
    }

    pub fn payload_bytes(mut self, data: &[u8]) -> Self {
        self.payload = Some(base64::Engine::encode(&base64::engine::general_purpose::STANDARD, data));
        self
    }

    pub fn meta(mut self, key: &str, value: &str) -> Self {
        let meta = self.meta.get_or_insert_with(HashMap::new);
        meta.insert(key.to_string(), value.to_string());
        self
    }

    pub fn meta_map(mut self, map: HashMap<String, String>) -> Self {
        self.meta = Some(map);
        self
    }
}
