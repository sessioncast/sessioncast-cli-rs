use crate::config::AgentConfig;
use crate::tmux::{self, PaneData};
use crate::websocket::{RelayWebSocketClient, WebSocketClientOptions, WsEvent};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::Sender;

const CAPTURE_INTERVAL_ACTIVE_MS: u64 = 50;
const CAPTURE_INTERVAL_IDLE_MS: u64 = 200;
const ACTIVE_THRESHOLD_MS: u64 = 2000;
const FORCE_SEND_INTERVAL_MS: u64 = 10000;
const MIN_COMPRESS_SIZE: usize = 512;
const META_CHECK_INTERVAL_MS: u64 = 15000;

/// Session handler for a single tmux session
pub struct SessionHandler {
    config: AgentConfig,
    tmux_session: String,
    session_id: String,
    ws_client: Option<Arc<RelayWebSocketClient>>,
    running: bool,
    last_screen: String,
    last_pane_screens: HashMap<String, String>,
    last_pane_ids: String,
    last_change_time: Instant,
    last_force_send_time: Instant,
    current_meta_json: String,
}

impl SessionHandler {
    pub fn new(config: AgentConfig, tmux_session: String) -> Self {
        let session_id = format!("{}/{}", config.machine_id, tmux_session);
        Self {
            config,
            tmux_session,
            session_id,
            ws_client: None,
            running: false,
            last_screen: String::new(),
            last_pane_screens: HashMap::new(),
            last_pane_ids: String::new(),
            last_change_time: Instant::now(),
            last_force_send_time: Instant::now(),
            current_meta_json: String::new(),
        }
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn tmux_session(&self) -> &str {
        &self.tmux_session
    }

    /// Start the session handler
    pub async fn start(&mut self, session_created_tx: Option<Sender<(String, Option<String>)>>) {
        if self.running {
            return;
        }
        self.running = true;

        // Add jitter
        let jitter = rand::random::<u64>() % 5000;
        tracing::info!("[{}] Starting in {}ms", self.tmux_session, jitter);
        tokio::time::sleep(Duration::from_millis(jitter)).await;

        let options = WebSocketClientOptions {
            url: self.config.relay.clone(),
            session_id: self.session_id.clone(),
            machine_id: self.config.machine_id.clone(),
            token: self.config.token.clone(),
            label: Some(self.tmux_session.clone()),
            auto_reconnect: true,
            enc_key: self.config.enc_key.as_ref().map(|k| {
                base64::Engine::decode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, k)
                    .unwrap_or_default()
            }),
            skip_auto_register: false,
        };

        let ws_client = Arc::new(RelayWebSocketClient::new(options));
        let mut event_rx = ws_client.subscribe();

        self.ws_client = Some(Arc::clone(&ws_client));
        ws_client.start().await;

        tracing::info!("[{}] Session handler started", self.tmux_session);

        // Main event loop
        while self.running {
            tokio::select! {
                // Handle WebSocket events
                Ok(event) = event_rx.recv() => {
                    self.handle_ws_event(event, &session_created_tx).await;
                }

                // Screen capture loop
                _ = tokio::time::sleep(Duration::from_millis(self.get_capture_interval())) => {
                    self.capture_and_send().await;
                }

                // Meta tracking
                _ = tokio::time::sleep(Duration::from_millis(META_CHECK_INTERVAL_MS)) => {
                    self.check_and_send_meta().await;
                }
            }
        }
    }

    fn get_capture_interval(&self) -> u64 {
        let elapsed = self.last_change_time.elapsed().as_millis() as u64;
        if elapsed < ACTIVE_THRESHOLD_MS {
            CAPTURE_INTERVAL_ACTIVE_MS
        } else {
            CAPTURE_INTERVAL_IDLE_MS
        }
    }

    async fn handle_ws_event(
        &mut self,
        event: WsEvent,
        session_created_tx: &Option<Sender<(String, Option<String>)>>,
    ) {
        match event {
            WsEvent::Connected => {
                tracing::info!("[{}] Connected to relay", self.tmux_session);
                self.current_meta_json.clear(); // Force resend
            }
            WsEvent::Disconnected { code, reason } => {
                tracing::info!("[{}] Disconnected: code={}, reason={}", self.tmux_session, code, reason);
            }
            WsEvent::Message(msg) => {
                self.handle_message(msg, session_created_tx).await;
            }
            WsEvent::Error(e) => {
                tracing::error!("[{}] WebSocket error: {}", self.tmux_session, e);
            }
        }
    }

    async fn handle_message(
        &mut self,
        msg: crate::websocket::Message,
        session_created_tx: &Option<Sender<(String, Option<String>)>>,
    ) {
        match msg.msg_type.as_str() {
            "keys" | "keysEnc" => {
                if let Some(keys) = &msg.payload {
                    let pane_id = msg.meta.as_ref().and_then(|m| m.get("pane").map(|s| s.as_str()));
                    self.handle_keys(keys, pane_id);
                }
            }
            "resize" => {
                if let Some(meta) = &msg.meta {
                    self.handle_resize(meta);
                }
            }
            "createSession" => {
                if let Some(meta) = &msg.meta {
                    if let Some(name) = meta.get("sessionName") {
                        tracing::info!("[{}] Create session request: {}", self.tmux_session, name);
                        if let Some(tx) = session_created_tx {
                            let _ = tx.send((name.clone(), None)).await;
                        }
                    }
                }
            }
            "killSession" => {
                tracing::info!("[{}] Kill session request", self.tmux_session);
                tmux::kill_session(&self.tmux_session);
                self.running = false;
            }
            "refreshScreen" => {
                self.force_refresh_screen().await;
            }
            _ => {}
        }
    }

    fn handle_keys(&self, keys: &str, pane_id: Option<&str>) {
        let target = pane_id.unwrap_or(&self.tmux_session);
        tmux::send_keys(target, keys, false);
    }

    fn handle_resize(&self, meta: &HashMap<String, String>) {
        let cols: usize = meta.get("cols").and_then(|s| s.parse().ok()).unwrap_or(0);
        let rows: usize = meta.get("rows").and_then(|s| s.parse().ok()).unwrap_or(0);

        if cols < 10 || rows < 4 {
            tracing::debug!(
                "[{}] Ignoring resize with too-small dimensions: {}x{}",
                self.tmux_session,
                cols,
                rows
            );
            return;
        }

        if meta.contains_key("pane") {
            tracing::debug!(
                "[{}] Ignoring pane resize (window-only policy)",
                self.tmux_session
            );
            return;
        }

        tracing::info!("[{}] Resize: {}x{}", self.tmux_session, cols, rows);
        tmux::resize_window(&self.tmux_session, cols, rows);
    }

    async fn capture_and_send(&mut self) {
        let Some(ws_client) = self.ws_client.clone() else {
            return;
        };

        if !ws_client.is_connected() {
            return;
        }

        let now = Instant::now();
        let force_send = now.duration_since(self.last_force_send_time).as_millis() as u64 >= FORCE_SEND_INTERVAL_MS;

        // Check for multi-pane
        if let Some(panes) = tmux::list_panes(&self.tmux_session) {
            if panes.len() > 1 {
                self.capture_multi_pane(&panes, &ws_client, force_send, now).await;
                return;
            }
        }

        // Single pane mode
        self.capture_single_pane(&ws_client, force_send, now).await;
    }

    async fn capture_single_pane(
        &mut self,
        ws_client: &RelayWebSocketClient,
        force_send: bool,
        now: Instant,
    ) {
        let Some(screen) = tmux::capture_pane(&self.tmux_session) else {
            return;
        };

        let changed = screen != self.last_screen;

        if changed || force_send {
            self.last_screen = screen.clone();
            self.last_force_send_time = now;

            if changed {
                self.last_change_time = now;
            }

            // Send clear screen + content
            let full_output = format!("\x1b[2J\x1b[H{}", screen);
            let data = full_output.as_bytes();

            if data.len() > MIN_COMPRESS_SIZE {
                ws_client.send_screen_compressed(
                    &self.session_id,
                    data,
                    self.config.enc_key.as_ref().map(|k| k.as_bytes()),
                );
            } else {
                ws_client.send_screen(&self.session_id, data);
            }
        }
    }

    async fn capture_multi_pane(
        &mut self,
        panes: &[PaneData],
        ws_client: &RelayWebSocketClient,
        force_send: bool,
        now: Instant,
    ) {
        let current_pane_ids: String = panes.iter().map(|p| p.id.as_str()).collect::<Vec<_>>().join(",");

        if current_pane_ids != self.last_pane_ids || force_send {
            self.last_pane_ids = current_pane_ids;
            ws_client.send_pane_layout(&self.session_id, panes);
        }

        for pane in panes {
            if let Some(screen) = tmux::capture_pane_by_id(&self.tmux_session, &pane.id) {
                let last_screen = self.last_pane_screens.get(&pane.id).cloned().unwrap_or_default();
                let changed = screen != last_screen;

                if changed || force_send {
                    self.last_pane_screens.insert(pane.id.clone(), screen.clone());

                    if changed {
                        self.last_change_time = now;
                    }

                    let full_output = format!("\x1b[2J\x1b[H{}", screen);
                    let data = full_output.as_bytes();

                    // Send with pane meta
                    let mut meta = HashMap::new();
                    meta.insert("pane".to_string(), pane.id.clone());

                    if data.len() > MIN_COMPRESS_SIZE {
                        // TODO: Implement send_screen_compressed_with_meta
                        ws_client.send_screen(&self.session_id, data);
                    } else {
                        ws_client.send_screen(&self.session_id, data);
                    }
                }
            }
        }

        if force_send {
            self.last_force_send_time = now;
        }
    }

    async fn check_and_send_meta(&mut self) {
        let Some(ws_client) = &self.ws_client else {
            return;
        };

        if !ws_client.is_connected() {
            return;
        }

        let Some(cwd) = tmux::get_pane_cwd(&self.tmux_session, None) else {
            return;
        };

        let git_info = tmux::get_git_info(&cwd);

        let mut meta = HashMap::new();
        meta.insert("cwd".to_string(), cwd);

        if let Some(git) = git_info {
            if let Some(branch) = git.branch {
                meta.insert("gitBranch".to_string(), branch);
            }
            if let Some(remote) = git.remote {
                meta.insert("gitRemote".to_string(), remote);
            }
            if let Some(repo) = git.repo {
                meta.insert("gitRepo".to_string(), repo);
            }
        }

        let json = serde_json::to_string(&meta).unwrap_or_default();

        if json != self.current_meta_json {
            self.current_meta_json = json.clone();
            ws_client.send_session_meta(&self.session_id, meta);
            tracing::info!("[{}] Session meta updated: {}", self.tmux_session, json);
        }
    }

    async fn force_refresh_screen(&mut self) {
        let Some(ws_client) = &self.ws_client else {
            return;
        };

        if !ws_client.is_connected() {
            return;
        }

        let Some(screen) = tmux::capture_pane(&self.tmux_session) else {
            return;
        };

        self.last_screen = screen.clone();
        let full_output = format!("\x1b[2J\x1b[H{}", screen);
        let data = full_output.as_bytes();

        if data.len() > MIN_COMPRESS_SIZE {
            ws_client.send_screen_compressed(
                &self.session_id,
                data,
                self.config.enc_key.as_ref().map(|k| k.as_bytes()),
            );
        } else {
            ws_client.send_screen(&self.session_id, data);
        }

        tracing::info!("[{}] Force refreshed screen", self.tmux_session);
    }

    pub fn stop(&mut self) {
        tracing::info!("[{}] Stopping", self.tmux_session);
        self.running = false;

        if let Some(ws_client) = &self.ws_client {
            ws_client.stop();
        }
    }
}
