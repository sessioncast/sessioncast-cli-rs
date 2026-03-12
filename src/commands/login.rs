use crate::config::AppConfig;
use anyhow::Result;
use base64::Engine;
use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use rand::RngCore;
use sha2::{Digest, Sha256};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use tokio::sync::oneshot;

const CLIENT_ID: &str = "sessioncast-cli";
const DEFAULT_SCOPES: &str = "openid profile email";

/// Login with API key or browser
pub async fn login(api_key: Option<&str>, api_url: Option<&str>, auth_url: Option<&str>) -> Result<()> {
    let mut config = AppConfig::load()?;

    // Set custom URLs if provided
    if let Some(url) = api_url {
        config.set_api_url(url.to_string());
        println!("{}", format!("API URL set to: {}", url).dimmed());
    }
    if let Some(url) = auth_url {
        config.set_auth_url(url.to_string());
    }

    // If API key provided, use manual login
    if let Some(key) = api_key {
        return manual_login(&mut config, key).await;
    }

    // Otherwise, use browser-based OAuth login
    browser_login(&mut config).await
}

async fn manual_login(config: &mut AppConfig, api_key: &str) -> Result<()> {
    // Validate API key format
    if !api_key.starts_with("sk-") && !api_key.starts_with("agt_") {
        anyhow::bail!("Invalid key format. Key should start with \"sk-\" or \"agt_\"");
    }

    if api_key.starts_with("agt_") {
        config.set_agent_token(api_key.to_string());
        println!("{}", "✓ Agent token saved!".green());
    } else {
        config.set_api_key(api_key.to_string());
        println!("{}", "✓ API key saved!".green());
    }

    config.save()?;
    Ok(())
}

async fn browser_login(config: &mut AppConfig) -> Result<()> {
    println!("{}", "\n  SessionCast Login\n".bold());

    let spinner = ProgressBar::new_spinner();
    spinner.set_style(ProgressStyle::default_spinner().template("{spinner} {msg}")?);
    spinner.set_message("Preparing login...");
    spinner.enable_steady_tick(std::time::Duration::from_millis(100));

    // Find available port
    let port = find_available_port(9876)?;
    let redirect_uri = format!("http://127.0.0.1:{}/callback", port);

    // Generate PKCE values
    let code_verifier = generate_code_verifier();
    let code_challenge = generate_code_challenge(&code_verifier);
    let state = generate_state();

    // Build authorization URL
    let mut auth_url_builder = url::Url::parse(&format!("{}/oauth/authorize", config.auth_url()))?;
    {
        let mut query = auth_url_builder.query_pairs_mut();
        query.append_pair("client_id", CLIENT_ID);
        query.append_pair("redirect_uri", &redirect_uri);
        query.append_pair("response_type", "code");
        query.append_pair("scope", DEFAULT_SCOPES);
        query.append_pair("state", &state);
        query.append_pair("code_challenge", &code_challenge);
        query.append_pair("code_challenge_method", "S256");
    }

    spinner.set_message("Opening browser...");

    // Start callback server
    let (tx, rx) = oneshot::channel();
    let _server_handle = tokio::spawn(async move {
        start_callback_server(port, tx).await
    });

    // Open browser
    match open::that(auth_url_builder.as_str()) {
        Ok(_) => {
            spinner.set_message("Waiting for authentication...");
            println!(
                "{}",
                format!(
                    "\n  If browser didn't open, visit:\n  {}\n",
                    auth_url_builder
                )
                .dimmed()
            );
        }
        Err(_) => {
            spinner.finish_with_message("Could not open browser");
            println!(
                "{}",
                format!(
                    "\nPlease open this URL in your browser:\n{}\n",
                    auth_url_builder
                )
                .yellow()
            );
        }
    }

    // Wait for callback
    let result = rx.await??;

    // Check for errors
    if let Some(error) = &result.error {
        let msg = format!("Authentication failed: {}", error);
        spinner.finish_with_message(msg);
        anyhow::bail!("Authentication failed: {}", error);
    }

    // Verify state
    if result.state.as_deref() != Some(&state) {
        spinner.finish_with_message("Security error: State mismatch");
        anyhow::bail!("Security error: State mismatch");
    }

    let Some(code) = &result.code else {
        spinner.finish_with_message("No authorization code received");
        anyhow::bail!("No authorization code received");
    };

    // Exchange code for token
    spinner.set_message("Exchanging code for token...");

    let token_response = exchange_code_for_token(
        config.auth_url(),
        code,
        &redirect_uri,
        &code_verifier,
    )
    .await?;

    // Save tokens
    config.set_access_token(token_response.access_token.clone(), token_response.expires_in);
    if let Some(refresh_token) = &token_response.refresh_token {
        config.set_refresh_token(refresh_token.clone());
    }

    spinner.set_message("Generating agent token...");

    // Generate agent token
    if let Ok(agent_token) = generate_agent_token(config.api_url(), &token_response.access_token).await {
        config.set_agent_token(agent_token.token);
        config.set_machine_id(agent_token.machine_id);
    }

    spinner.finish_with_message("Login successful!");

    println!("{}", "\n✓ You are now logged in to SessionCast\n".green());
    println!("{}", "  Run `sessioncast agent` to start the agent".dimmed());
    println!("{}", "  Run `sessioncast status` to check your login status\n".dimmed());

    config.save()?;
    Ok(())
}

#[derive(Debug)]
struct CallbackResult {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

async fn start_callback_server(port: u16, tx: oneshot::Sender<Result<CallbackResult>>) -> Result<()> {
    let listener = TcpListener::bind(format!("127.0.0.1:{}", port))?;
    listener.set_nonblocking(true)?;

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let result = handle_callback_request(stream);
                let _ = tx.send(result);
                return Ok(());
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                continue;
            }
            Err(e) => {
                let _ = tx.send(Err(e.into()));
                return Ok(());
            }
        }
    }

    let _ = tx.send(Err(anyhow::anyhow!("No callback received")));
    Ok(())
}

fn handle_callback_request(mut stream: std::net::TcpStream) -> Result<CallbackResult> {
    let mut reader = BufReader::new(&stream);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;

    let url_part = request_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("Invalid request"))?;

    let url = url::Url::parse(&format!("http://localhost{}", url_part))?;
    let query: std::collections::HashMap<String, String> = url
        .query_pairs()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();

    let result = CallbackResult {
        code: query.get("code").cloned(),
        state: query.get("state").cloned(),
        error: query.get("error").cloned(),
        error_description: query.get("error_description").cloned(),
    };

    // Send response
    let response = if result.error.is_some() {
        "HTTP/1.1 400 Bad Request\r\nContent-Type: text/html\r\n\r\n<h1>Authentication Failed</h1><p>You can close this window.</p>"
    } else {
        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n<h1>Authentication Successful</h1><p>You can close this window.</p>"
    };

    stream.write_all(response.as_bytes())?;
    stream.flush()?;

    Ok(result)
}

fn find_available_port(start: u16) -> Result<u16> {
    for port in start..start + 100 {
        if TcpListener::bind(format!("127.0.0.1:{}", port)).is_ok() {
            return Ok(port);
        }
    }
    anyhow::bail!("Could not find an available port")
}

fn generate_code_verifier() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn generate_code_challenge(verifier: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(verifier.as_bytes());
    let hash = hasher.finalize();
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(hash)
}

fn generate_state() -> String {
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

#[derive(Debug, serde::Deserialize)]
struct TokenResponse {
    access_token: String,
    token_type: String,
    expires_in: u64,
    refresh_token: Option<String>,
    scope: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct AgentTokenResponse {
    token: String,
    machine_id: String,
}

async fn exchange_code_for_token(
    auth_url: &str,
    code: &str,
    redirect_uri: &str,
    code_verifier: &str,
) -> Result<TokenResponse> {
    let client = reqwest::Client::new();

    let params = [
        ("grant_type", "authorization_code".to_string()),
        ("client_id", CLIENT_ID.to_string()),
        ("code", code.to_string()),
        ("redirect_uri", redirect_uri.to_string()),
        ("code_verifier", code_verifier.to_string()),
    ];

    let response = client
        .post(&format!("{}/oauth/token", auth_url))
        .form(&params)
        .send()
        .await?;

    if !response.status().is_success() {
        let text = response.text().await?;
        anyhow::bail!("Token exchange failed: {}", text);
    }

    Ok(response.json().await?)
}

async fn generate_agent_token(api_url: &str, access_token: &str) -> Result<AgentTokenResponse> {
    let client = reqwest::Client::new();

    let hostname = hostname::get()
        .map(|h| h.to_string_lossy().to_string())
        .unwrap_or_else(|_| "unknown".to_string());

    let response = client
        .post(&format!("{}/api/tokens/generate", api_url))
        .bearer_auth(access_token)
        .json(&serde_json::json!({ "machineId": hostname }))
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(anyhow::anyhow!("Failed to generate agent token"));
    }

    Ok(response.json().await?)
}

mod hostname {
    use std::process::Command;

    pub fn get() -> std::io::Result<std::ffi::OsString> {
        #[cfg(unix)]
        {
            let output = Command::new("hostname").output()?;
            Ok(String::from_utf8_lossy(&output.stdout).trim().to_string().into())
        }
        #[cfg(windows)]
        {
            let output = Command::new("hostname").output()?;
            Ok(String::from_utf8_lossy(&output.stdout).trim().to_string().into())
        }
    }
}

/// Logout
pub fn logout() -> Result<()> {
    let config = AppConfig::load()?;

    if !config.is_logged_in() {
        println!("{}", "Not logged in.".yellow());
        return Ok(());
    }

    let mut config = config;
    config.clear_auth();
    config.save()?;

    println!("{}", "✓ Logged out successfully!".green());
    Ok(())
}

/// Show login status
pub fn status() -> Result<()> {
    let config = AppConfig::load()?;

    if config.is_logged_in() {
        println!("{}", "✓ Logged in".green());

        if config.access_token().is_some() {
            println!("{}", "  Auth method: OAuth".dimmed());
        } else {
            println!("{}", "  Auth method: API Key / Agent Token".dimmed());
        }
    } else {
        println!("{}", "Not logged in".yellow());
        println!("{}", "Run: sessioncast login".dimmed());
    }

    Ok(())
}
