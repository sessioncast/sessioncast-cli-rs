use crate::agent::SessionHandler;
use crate::config::AgentConfig;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tokio::time::{interval, Duration};

const SCAN_INTERVAL_MS: u64 = 5000;

/// Agent runner - manages multiple session handlers
pub struct AgentRunner {
    config: AgentConfig,
    handlers: HashMap<String, Arc<tokio::sync::Mutex<SessionHandler>>>,
    running: bool,
}

impl AgentRunner {
    pub fn new(config: AgentConfig) -> Self {
        Self {
            config,
            handlers: HashMap::new(),
            running: false,
        }
    }

    /// Load agent configuration
    pub async fn load_config(config_path: Option<&str>) -> anyhow::Result<AgentConfig> {
        AgentConfig::load(config_path).await
    }

    /// Start the agent
    pub async fn start(&mut self) -> anyhow::Result<()> {
        if self.running {
            return Ok(());
        }
        self.running = true;

        tracing::info!("Starting SessionCast Agent...");
        tracing::info!("Machine ID: {}", self.config.machine_id);
        tracing::info!("Relay: {}", self.config.relay);
        tracing::info!("Token: {}", if self.config.token.is_empty() { "none" } else { "present" });
        tracing::info!("E2E Encryption: {}", if self.config.enc_key.is_some() { "enabled" } else { "disabled" });

        // Check tmux availability
        if !crate::tmux::is_available() {
            return Err(anyhow::anyhow!("{}", self.get_tmux_not_found_error()));
        }

        // Channel for session creation requests
        let (session_created_tx, _session_created_rx): (
            Sender<(String, Option<String>)>,
            Receiver<(String, Option<String>)>,
        ) = channel(10);

        // Initial scan
        self.scan_and_update_sessions(Some(session_created_tx.clone())).await;

        // Spawn periodic scanner
        let _scan_config = self.config.clone();
        let _scan_tx = session_created_tx.clone();
        tokio::spawn(async move {
            let mut scan_interval = interval(Duration::from_millis(SCAN_INTERVAL_MS));
            loop {
                scan_interval.tick().await;
                // Scanner logic would go here
            }
        });

        tracing::info!("Agent started with auto-discovery (scanning every {}s)", SCAN_INTERVAL_MS / 1000);

        // Wait for shutdown signal
        tokio::signal::ctrl_c().await?;
        tracing::info!("Shutting down Agent...");
        self.stop().await;

        Ok(())
    }

    async fn scan_and_update_sessions(&mut self, session_created_tx: Option<Sender<(String, Option<String>)>>) {
        let current_sessions: std::collections::HashSet<String> =
            crate::tmux::scan_sessions().into_iter().collect();

        let tracked_sessions: std::collections::HashSet<String> =
            self.handlers.keys().cloned().collect();

        // Start handlers for new sessions
        for session in current_sessions.difference(&tracked_sessions) {
            self.start_session_handler(session, session_created_tx.clone()).await;
        }

        // Stop handlers for removed sessions
        for session in tracked_sessions.difference(&current_sessions) {
            self.stop_session_handler(session).await;
        }
    }

    async fn start_session_handler(
        &mut self,
        tmux_session: &str,
        session_created_tx: Option<Sender<(String, Option<String>)>>,
    ) {
        tracing::info!("Discovered new tmux session: {}", tmux_session);

        let config = self.config.clone();
        let session_name = tmux_session.to_string();
        let tx = session_created_tx.clone();

        let handler = Arc::new(tokio::sync::Mutex::new(SessionHandler::new(
            config,
            session_name.clone(),
        )));

        self.handlers.insert(tmux_session.to_string(), Arc::clone(&handler));

        let handler_clone = Arc::clone(&handler);
        tokio::spawn(async move {
            let mut h = handler_clone.lock().await;
            h.start(tx).await;
        });

        tracing::info!("Started handler for session: {}/{}", self.config.machine_id, tmux_session);
    }

    async fn stop_session_handler(&mut self, tmux_session: &str) {
        tracing::info!("Tmux session removed: {}", tmux_session);

        if let Some(handler) = self.handlers.remove(tmux_session) {
            let mut h = handler.lock().await;
            h.stop();
            tracing::info!(
                "Stopped handler for session: {}/{}",
                self.config.machine_id,
                tmux_session
            );
        }
    }

    fn get_tmux_not_found_error(&self) -> String {
        #[cfg(windows)]
        {
            r#"━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  itmux not found - Windows tmux package required
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

  Download: https://github.com/itefixnet/itmux/releases/latest
  Or: choco install itmux

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"#
            .to_string()
        }

        #[cfg(target_os = "macos")]
        {
            r#"━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  tmux not found - required for SessionCast Agent
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

  Install: brew install tmux

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"#
            .to_string()
        }

        #[cfg(not(any(windows, target_os = "macos")))]
        {
            r#"━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  tmux not found - required for SessionCast Agent
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

  Install: sudo apt install tmux  (Debian/Ubuntu)
           sudo yum install tmux  (RHEL/CentOS)

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"#
            .to_string()
        }
    }

    async fn stop(&mut self) {
        self.running = false;

        for (name, handler) in &self.handlers {
            let mut h = handler.lock().await;
            h.stop();
            tracing::info!("Stopped handler: {}", name);
        }

        self.handlers.clear();
        tracing::info!("Agent shutdown complete");
    }
}
