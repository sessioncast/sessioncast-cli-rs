use crate::crypto;
use crate::websocket::Message;
use flate2::write::GzEncoder;
use flate2::Compression;
use futures_util::{
    sink::SinkExt,
    stream::{SplitSink, SplitStream, StreamExt},
};
use std::collections::HashMap;
use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::sync::broadcast;
use tokio::time::sleep;
use tokio_tungstenite::{
    connect_async, tungstenite::protocol::Message as WsMessage, MaybeTlsStream, WebSocketStream,
};

const MAX_RECONNECT_ATTEMPTS: u32 = 5;
const BASE_RECONNECT_DELAY_MS: u64 = 2000;
const MAX_RECONNECT_DELAY_MS: u64 = 60000;
const CIRCUIT_BREAKER_DURATION_MS: u64 = 120000;

type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// WebSocket client options
#[derive(Debug, Clone)]
pub struct WebSocketClientOptions {
    pub url: String,
    pub session_id: String,
    pub machine_id: String,
    pub token: String,
    pub label: Option<String>,
    pub auto_reconnect: bool,
    pub enc_key: Option<Vec<u8>>,
    pub skip_auto_register: bool,
}

impl Default for WebSocketClientOptions {
    fn default() -> Self {
        Self {
            url: String::new(),
            session_id: String::new(),
            machine_id: String::new(),
            token: String::new(),
            label: None,
            auto_reconnect: true,
            enc_key: None,
            skip_auto_register: false,
        }
    }
}

/// Event from WebSocket
#[derive(Debug, Clone)]
pub enum WsEvent {
    Connected,
    Disconnected { code: u16, reason: String },
    Message(Message),
    Error(String),
}

/// Relay WebSocket client with reconnection
pub struct RelayWebSocketClient {
    options: WebSocketClientOptions,
    msg_tx: broadcast::Sender<Message>,
    event_tx: broadcast::Sender<WsEvent>,
    connected: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
    reconnect_attempts: Arc<AtomicU32>,
}

impl RelayWebSocketClient {
    pub fn new(options: WebSocketClientOptions) -> Self {
        let (msg_tx, _) = broadcast::channel(256);
        let (event_tx, _) = broadcast::channel(100);

        Self {
            options,
            msg_tx,
            event_tx,
            connected: Arc::new(AtomicBool::new(false)),
            running: Arc::new(AtomicBool::new(false)),
            reconnect_attempts: Arc::new(AtomicU32::new(0)),
        }
    }

    /// Get event receiver
    pub fn subscribe(&self) -> broadcast::Receiver<WsEvent> {
        self.event_tx.subscribe()
    }

    /// Get message sender
    pub fn sender(&self) -> broadcast::Sender<Message> {
        self.msg_tx.clone()
    }

    /// Check if connected
    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }

    /// Start the client
    pub async fn start(&self) {
        if self.running.swap(true, Ordering::SeqCst) {
            return;
        }

        let url = self.options.url.clone();
        let options = self.options.clone();
        let tx = self.msg_tx.clone();
        let event_tx = self.event_tx.clone();
        let connected = self.connected.clone();
        let running = self.running.clone();
        let reconnect_attempts = self.reconnect_attempts.clone();

        tokio::spawn(async move {
            let mut circuit_breaker_until: Option<u64> = None;

            while running.load(Ordering::Relaxed) {
                // Check circuit breaker
                if let Some(until) = circuit_breaker_until {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_millis() as u64)
                        .unwrap_or(0);

                    if now < until {
                        let remaining = until - now;
                        tracing::warn!("Circuit breaker open. Retry in {}s", remaining / 1000);
                        sleep(Duration::from_millis(remaining)).await;
                        reconnect_attempts.store(0, Ordering::Relaxed);
                        circuit_breaker_until = None;
                        continue;
                    }
                }

                // Try to connect
                match Self::run_connection(&url, &options, &tx, &event_tx, &connected).await {
                    Ok(code) => {
                        tracing::info!("WebSocket closed with code: {}", code);
                        
                        if !options.auto_reconnect || !running.load(Ordering::Relaxed) {
                            break;
                        }

                        // Check for limit exceeded
                        if code == 1013 {
                            tracing::error!("Session limit exceeded");
                            running.store(false, Ordering::Relaxed);
                            break;
                        }

                        // Increment reconnect attempts
                        let attempts = reconnect_attempts.fetch_add(1, Ordering::Relaxed) + 1;
                        
                        if attempts > MAX_RECONNECT_ATTEMPTS {
                            tracing::error!(
                                "Max reconnect attempts reached. Circuit breaker active for {}s",
                                CIRCUIT_BREAKER_DURATION_MS / 1000
                            );
                            let now = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .map(|d| d.as_millis() as u64)
                                .unwrap_or(0);
                            circuit_breaker_until = Some(now + CIRCUIT_BREAKER_DURATION_MS);
                            reconnect_attempts.store(0, Ordering::Relaxed);
                            continue;
                        }

                        // Exponential backoff with jitter
                        let delay = std::cmp::min(
                            BASE_RECONNECT_DELAY_MS * 2u64.pow(attempts - 1),
                            MAX_RECONNECT_DELAY_MS,
                        );
                        let jitter = (rand::random::<u64>() % delay) / 2;
                        let reconnect_delay = delay + jitter;

                        tracing::info!(
                            "Reconnecting in {}ms (attempt {}/{})",
                            reconnect_delay,
                            attempts,
                            MAX_RECONNECT_ATTEMPTS
                        );

                        sleep(Duration::from_millis(reconnect_delay)).await;
                    }
                    Err(e) => {
                        tracing::error!("WebSocket error: {}", e);
                        
                        if !options.auto_reconnect || !running.load(Ordering::Relaxed) {
                            break;
                        }

                        let attempts = reconnect_attempts.fetch_add(1, Ordering::Relaxed) + 1;
                        let delay = std::cmp::min(
                            BASE_RECONNECT_DELAY_MS * 2u64.pow(attempts - 1),
                            MAX_RECONNECT_DELAY_MS,
                        );

                        sleep(Duration::from_millis(delay)).await;
                    }
                }
            }
        });
    }

    async fn run_connection(
        url: &str,
        options: &WebSocketClientOptions,
        msg_tx: &broadcast::Sender<Message>,
        event_tx: &broadcast::Sender<WsEvent>,
        connected: &Arc<AtomicBool>,
    ) -> Result<u16, anyhow::Error> {
        tracing::info!("Connecting to {}", url);

        let (ws_stream, _) = connect_async(url).await?;
        let (mut write, read) = ws_stream.split();

        connected.store(true, Ordering::Relaxed);
        let _ = event_tx.send(WsEvent::Connected);
        tracing::info!("WebSocket connected");

        // Register as host
        if !options.skip_auto_register {
            let register = Self::create_register_message(options);
            let json = serde_json::to_string(&register)?;
            write.send(WsMessage::Text(json)).await?;
        }

        // Read messages (write is handled via send on msg_tx elsewhere)
        let result = Self::read_messages(read, event_tx, &options.enc_key, msg_tx.clone(), write, connected.clone()).await;

        // Cleanup
        connected.store(false, Ordering::Relaxed);

        result
    }

    async fn read_messages(
        mut read: SplitStream<WsStream>,
        event_tx: &broadcast::Sender<WsEvent>,
        enc_key: &Option<Vec<u8>>,
        msg_tx: broadcast::Sender<Message>,
        mut write: SplitSink<WsStream, WsMessage>,
        connected: Arc<AtomicBool>,
    ) -> Result<u16, anyhow::Error> {
        let mut rx = msg_tx.subscribe();
        
        loop {
            tokio::select! {
                // Read incoming messages
                msg = read.next() => {
                    match msg {
                        Some(Ok(WsMessage::Text(text))) => {
                            if let Ok(message) = serde_json::from_str::<Message>(&text) {
                                let message = Self::decrypt_if_needed(message, enc_key);
                                let _ = event_tx.send(WsEvent::Message(message));
                            }
                        }
                        Some(Ok(WsMessage::Close(frame))) => {
                            let (code, reason) = frame
                                .map(|f| (f.code.into(), f.reason.to_string()))
                                .unwrap_or((1000, "Normal closure".to_string()));
                            let _ = event_tx.send(WsEvent::Disconnected { code, reason: reason.clone() });
                            return Ok(code);
                        }
                        Some(Ok(WsMessage::Ping(_))) => {
                            // Pong is handled automatically
                        }
                        Some(Err(e)) => {
                            let _ = event_tx.send(WsEvent::Error(e.to_string()));
                            return Err(e.into());
                        }
                        None => return Ok(1000),
                        _ => {}
                    }
                }
                // Write outgoing messages
                msg = rx.recv() => {
                    if !connected.load(Ordering::Relaxed) {
                        break;
                    }
                    if let Ok(msg) = msg {
                        let json = serde_json::to_string(&msg)?;
                        if write.send(WsMessage::Text(json)).await.is_err() {
                            break;
                        }
                    }
                }
            }
        }

        Ok(1000)
    }

    fn create_register_message(options: &WebSocketClientOptions) -> Message {
        let mut meta = HashMap::new();
        meta.insert("label".to_string(), options.label.clone().unwrap_or_else(|| options.session_id.clone()));
        meta.insert("machineId".to_string(), options.machine_id.clone());
        if !options.token.is_empty() {
            meta.insert("token".to_string(), options.token.clone());
        }

        Message::new("register")
            .role("host")
            .session(&options.session_id)
            .meta_map(meta)
    }

    fn decrypt_if_needed(message: Message, enc_key: &Option<Vec<u8>>) -> Message {
        if message.msg_type != "keysEnc" {
            return message;
        }

        let key = match enc_key {
            Some(k) => k,
            None => return message,
        };

        let payload = match &message.payload {
            Some(p) => p,
            None => return message,
        };

        // Decode base64
        let encrypted = match base64::Engine::decode(&base64::engine::general_purpose::STANDARD, payload) {
            Ok(d) => d,
            Err(_) => return message,
        };

        // Decrypt
        match crypto::decrypt(&encrypted, key) {
            Ok(decrypted) => {
                let mut msg = message;
                msg.msg_type = "keys".to_string();
                msg.payload = Some(String::from_utf8_lossy(&decrypted).to_string());
                msg
            }
            Err(_) => message,
        }
    }

    /// Stop the client
    pub fn stop(&self) {
        self.running.store(false, Ordering::Relaxed);
        self.connected.store(false, Ordering::Relaxed);
    }

    /// Send screen data
    pub fn send_screen(&self, session: &str, data: &[u8]) {
        let msg = Message::new("screen")
            .session(session)
            .payload_bytes(data);
        let _ = self.msg_tx.send(msg);
    }

    /// Send compressed screen data
    pub fn send_screen_compressed(&self, session: &str, data: &[u8], enc_key: Option<&[u8]>) {
        // Try zstd compression, fallback to gzip
        let compressed = zstd::encode_all(data, 3).unwrap_or_else(|_| {
            let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
            encoder.write_all(data).ok();
            encoder.finish().unwrap_or_default()
        });

        // Encrypt if key available
        let (payload, msg_type) = if let Some(key) = enc_key {
            match crypto::decrypt(&compressed, key) {
                Ok(encrypted) => (encrypted, "screenEnc"),
                Err(_) => (compressed, "screenZstd"),
            }
        } else {
            (compressed, "screenZstd")
        };

        let msg = Message::new(msg_type)
            .session(session)
            .payload_bytes(&payload);
        let _ = self.msg_tx.send(msg);
    }

    /// Send session metadata
    pub fn send_session_meta(&self, session: &str, meta: HashMap<String, String>) {
        let msg = Message::new("sessionMeta")
            .session(session)
            .meta_map(meta);
        let _ = self.msg_tx.send(msg);
    }

    /// Send pane layout
    pub fn send_pane_layout(&self, session: &str, panes: &[crate::tmux::PaneData]) {
        let panes_json = serde_json::to_string(panes).unwrap_or_default();
        let msg = Message::new("paneLayout")
            .session(session)
            .payload(&panes_json);
        let _ = self.msg_tx.send(msg);
    }
}
