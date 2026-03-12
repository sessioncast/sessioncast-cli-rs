use crate::tmux::{GitInfo, PaneData, TmuxSession};
use std::path::Path;
use std::process::Command;

/// Trait for tmux operations
pub trait TmuxExecutor: Send + Sync {
    fn list_sessions(&self) -> Vec<String>;
    fn capture_pane(&self, session: &str) -> Option<String>;
    fn capture_pane_by_id(&self, session: &str, pane_id: &str) -> Option<String>;
    fn list_panes(&self, session: &str) -> Option<Vec<PaneData>>;
    fn send_keys(&self, session: &str, keys: &str) -> bool;
    fn send_special_key(&self, session: &str, key: &str) -> bool;
    fn resize_window(&self, session: &str, cols: usize, rows: usize) -> bool;
    fn resize_pane(&self, pane_id: &str, cols: usize, rows: usize) -> bool;
    fn kill_session(&self, session: &str) -> bool;
    fn create_session(&self, session: &str, working_dir: Option<&str>) -> bool;
    fn is_available(&self) -> bool;
    fn get_version(&self) -> Option<String>;
    fn get_active_pane(&self, session: &str) -> Option<String>;
    fn get_pane_cwd(&self, session: &str, pane_id: Option<&str>) -> Option<String>;
}

/// Create the appropriate executor for the current platform
pub fn create_executor() -> Box<dyn TmuxExecutor> {
    #[cfg(windows)]
    {
        Box::new(WindowsTmuxExecutor::new())
    }
    #[cfg(not(windows))]
    {
        Box::new(UnixTmuxExecutor::new())
    }
}

/// Check if tmux is available
pub fn is_available() -> bool {
    create_executor().is_available()
}

// ============================================================================
// Unix Implementation
// ============================================================================

#[cfg(not(windows))]
pub struct UnixTmuxExecutor;

#[cfg(not(windows))]
impl Default for UnixTmuxExecutor {
    fn default() -> Self {
        Self::new()
    }
}

impl UnixTmuxExecutor {
    pub fn new() -> Self {
        Self
    }

    fn execute(&self, args: &[&str]) -> Option<String> {
        Command::new("tmux")
            .args(args)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
    }

    fn escape_for_shell(s: &str) -> String {
        s.replace('"', "\\\"")
            .replace('$', "\\$")
            .replace('`', "\\`")
    }
}

#[cfg(not(windows))]
impl TmuxExecutor for UnixTmuxExecutor {
    fn list_sessions(&self) -> Vec<String> {
        self.execute(&["ls", "-F", "#{session_name}"])
            .map(|s| {
                s.lines()
                    .map(|l| l.trim().to_string())
                    .filter(|l| !l.is_empty())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn capture_pane(&self, session: &str) -> Option<String> {
        self.execute(&["capture-pane", "-t", session, "-p", "-e", "-N"])
            .map(|s| s.replace('\n', "\r\n"))
    }

    fn capture_pane_by_id(&self, _session: &str, pane_id: &str) -> Option<String> {
        self.execute(&["capture-pane", "-t", pane_id, "-p", "-e", "-N"])
            .map(|s| s.replace('\n', "\r\n"))
    }

    fn list_panes(&self, session: &str) -> Option<Vec<PaneData>> {
        let output = self.execute(&[
            "list-panes",
            "-t",
            session,
            "-F",
            "#{pane_id}:#{pane_index}:#{pane_width}:#{pane_height}:#{pane_top}:#{pane_left}:#{?pane_active,1,0}:#{pane_title}",
        ])?;

        let panes = output
            .lines()
            .filter(|l| !l.is_empty())
            .filter_map(|line| {
                let parts: Vec<&str> = line.split(':').collect();
                if parts.len() >= 8 {
                    let title_parts: Vec<&str> = parts[7..].to_vec();
                    Some(PaneData {
                        id: parts[0].to_string(),
                        index: parts[1].parse().ok()?,
                        width: parts[2].parse().ok()?,
                        height: parts[3].parse().ok()?,
                        top: parts[4].parse().ok()?,
                        left: parts[5].parse().ok()?,
                        active: parts[6] == "1",
                        title: title_parts.join(":"),
                    })
                } else {
                    None
                }
            })
            .collect();

        Some(panes)
    }

    fn send_keys(&self, session: &str, keys: &str) -> bool {
        let escaped = Self::escape_for_shell(keys);
        self.execute(&["send-keys", "-t", session, "-l", &escaped])
            .is_some()
    }

    fn send_special_key(&self, session: &str, key: &str) -> bool {
        self.execute(&["send-keys", "-t", session, key])
            .is_some()
    }

    fn resize_window(&self, session: &str, cols: usize, rows: usize) -> bool {
        self.execute(&[
            "resize-window",
            "-t",
            session,
            "-x",
            &cols.to_string(),
            "-y",
            &rows.to_string(),
        ])
        .is_some()
    }

    fn resize_pane(&self, pane_id: &str, cols: usize, rows: usize) -> bool {
        self.execute(&[
            "resize-pane",
            "-t",
            pane_id,
            "-x",
            &cols.to_string(),
            "-y",
            &rows.to_string(),
        ])
        .is_some()
    }

    fn kill_session(&self, session: &str) -> bool {
        self.execute(&["kill-session", "-t", session]).is_some()
    }

    fn create_session(&self, session: &str, working_dir: Option<&str>) -> bool {
        let sanitized: String = session
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
            .collect();

        if sanitized.is_empty() {
            return false;
        }

        let args = vec!["new-session", "-d", "-s", &sanitized];
        let mut cwd_arg = None;

        if let Some(cwd) = working_dir {
            cwd_arg = Some(format!("-c {}", cwd));
        }

        let full_args: Vec<&str> = if let Some(_cwd) = cwd_arg {
            let mut a = args.clone();
            a.push("-c");
            a.push(working_dir.unwrap());
            a
        } else {
            args
        };

        self.execute(&full_args).is_some()
    }

    fn is_available(&self) -> bool {
        which::which("tmux").is_ok()
    }

    fn get_version(&self) -> Option<String> {
        self.execute(&["-V"]).map(|s| s.trim().to_string())
    }

    fn get_active_pane(&self, session: &str) -> Option<String> {
        let output = self.execute(&["display-message", "-t", session, "-p", "#{pane_id}"])?;
        let trimmed = output.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    }

    fn get_pane_cwd(&self, session: &str, pane_id: Option<&str>) -> Option<String> {
        let target = if let Some(pid) = pane_id {
            format!("{}:{}", session, pid)
        } else {
            session.to_string()
        };

        let output =
            self.execute(&["display-message", "-t", &target, "-p", "#{pane_current_path}"])?;
        let trimmed = output.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    }
}

// ============================================================================
// Windows Implementation (itmux)
// ============================================================================

#[cfg(windows)]
pub struct WindowsTmuxExecutor {
    itmux_path: Option<String>,
    bash_path: Option<String>,
}

#[cfg(windows)]
impl WindowsTmuxExecutor {
    pub fn new() -> Self {
        let itmux_path = Self::find_itmux_path();
        let bash_path = itmux_path
            .as_ref()
            .map(|p| format!("{}\\bin\\bash.exe", p));

        if let (Some(ref itmux), Some(ref bash)) = (&itmux_path, &bash_path) {
            tracing::info!("[Windows] Using itmux at: {}", itmux);
            tracing::debug!("Bash path: {}", bash);
        }

        Self {
            itmux_path,
            bash_path,
        }
    }

    fn find_itmux_path() -> Option<String> {
        // Check environment variable first
        if let Ok(path) = std::env::var("ITMUX_HOME") {
            if Self::check_bash_path(&path) {
                return Some(path);
            }
        }

        // Check PATH
        if let Ok(path_env) = std::env::var("PATH") {
            let sep = if path_env.contains(';') { ';' } else { ':' };
            for dir in path_env.split(sep) {
                let lower = dir.to_lowercase();
                if lower.contains("itmux") || lower.contains("cygwin") {
                    let check_path = if dir.ends_with("\\bin") || dir.ends_with("/bin") {
                        std::path::Path::new(dir)
                            .parent()
                            .map(|p| p.to_string_lossy().to_string())
                            .unwrap_or_else(|| dir.to_string())
                    } else {
                        dir.to_string()
                    };
                    if Self::check_bash_path(&check_path) {
                        return Some(check_path);
                    }
                }
            }
        }

        // Check common locations
        let locations = vec![
            format!("{}\\itmux", std::env::var("USERPROFILE").unwrap_or_default()),
            "C:\\itmux".to_string(),
            "D:\\itmux".to_string(),
            format!(
                "{}\\itmux",
                std::env::var("LOCALAPPDATA").unwrap_or_default()
            ),
        ];

        for loc in locations {
            if Self::check_bash_path(&loc) {
                return Some(loc);
            }
        }

        None
    }

    fn check_bash_path(base_path: &str) -> bool {
        std::path::Path::new(&format!("{}\\bin\\bash.exe", base_path)).exists()
    }

    fn execute(&self, command: &str) -> Option<String> {
        let bash = self.bash_path.as_ref()?;
        let username = whoami::username();

        let output = Command::new(bash)
            .args(["-l", "-c", command])
            .env("CYGWIN", "nodosfilewarning")
            .env("HOME", format!("/home/{}", username))
            .env("TERM", "xterm-256color")
            .current_dir(self.itmux_path.as_ref()?)
            .output()
            .ok()?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        Some(stdout)
    }

    fn escape_session(s: &str) -> String {
        s.replace('\'', "'\\''")
    }

    fn windows_to_cygwin_path(windows_path: &str) -> String {
        if windows_path.starts_with("\\\\") {
            return windows_path.to_string();
        }

        if windows_path.len() >= 2 && windows_path.chars().nth(1) == Some(':') {
            let drive = windows_path.chars().next().unwrap().to_ascii_lowercase();
            let rest = &windows_path[2..].replace('\\', "/");
            return format!("/cygdrive/{}{}", drive, rest);
        }

        windows_path.replace('\\', "/")
    }
}

#[cfg(windows)]
impl TmuxExecutor for WindowsTmuxExecutor {
    fn list_sessions(&self) -> Vec<String> {
        self.execute("tmux ls -F '#{session_name}' 2>/dev/null || true")
            .map(|s| {
                s.lines()
                    .map(|l| l.trim().to_string())
                    .filter(|l| !l.is_empty() && !l.starts_with("no server"))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn capture_pane(&self, session: &str) -> Option<String> {
        let escaped = Self::escape_session(session);
        self.execute(&format!("tmux capture-pane -t '{}' -p -e -N", escaped))
            .map(|s| s.replace('\n', "\r\n"))
    }

    fn capture_pane_by_id(&self, _session: &str, pane_id: &str) -> Option<String> {
        let escaped = Self::escape_session(pane_id);
        self.execute(&format!("tmux capture-pane -t '{}' -p -e -N", escaped))
            .map(|s| s.replace('\n', "\r\n"))
    }

    fn list_panes(&self, session: &str) -> Option<Vec<PaneData>> {
        let escaped = Self::escape_session(session);
        let output = self.execute(&format!(
            "tmux list-panes -t '{}' -F \"#{{pane_id}}:#{{pane_index}}:#{{pane_width}}:#{{pane_height}}:#{{pane_top}}:#{{pane_left}}:#{{?pane_active,1,0}}:#{{pane_title}}\"",
            escaped
        ))?;

        let panes = output
            .lines()
            .filter(|l| !l.is_empty())
            .filter_map(|line| {
                let parts: Vec<&str> = line.split(':').collect();
                if parts.len() >= 8 {
                    let title_parts: Vec<&str> = parts[7..].to_vec();
                    Some(PaneData {
                        id: parts[0].to_string(),
                        index: parts[1].parse().ok()?,
                        width: parts[2].parse().ok()?,
                        height: parts[3].parse().ok()?,
                        top: parts[4].parse().ok()?,
                        left: parts[5].parse().ok()?,
                        active: parts[6] == "1",
                        title: title_parts.join(":"),
                    })
                } else {
                    None
                }
            })
            .collect();

        Some(panes)
    }

    fn send_keys(&self, session: &str, keys: &str) -> bool {
        let escaped_session = Self::escape_session(session);
        let escaped_keys = keys.replace('\'', "'\\''");
        self.execute(&format!(
            "tmux send-keys -t '{}' -l '{}'",
            escaped_session, escaped_keys
        ))
        .is_some()
    }

    fn send_special_key(&self, session: &str, key: &str) -> bool {
        let escaped = Self::escape_session(session);
        self.execute(&format!("tmux send-keys -t '{}' {}", escaped, key))
            .is_some()
    }

    fn resize_window(&self, session: &str, cols: usize, rows: usize) -> bool {
        let escaped = Self::escape_session(session);
        self.execute(&format!(
            "tmux resize-window -t '{}' -x {} -y {}",
            escaped, cols, rows
        ))
        .is_some()
    }

    fn resize_pane(&self, pane_id: &str, cols: usize, rows: usize) -> bool {
        let escaped = Self::escape_session(pane_id);
        self.execute(&format!(
            "tmux resize-pane -t '{}' -x {} -y {}",
            escaped, cols, rows
        ))
        .is_some()
    }

    fn kill_session(&self, session: &str) -> bool {
        let escaped = Self::escape_session(session);
        self.execute(&format!("tmux kill-session -t '{}'", escaped))
            .is_some()
    }

    fn create_session(&self, session: &str, working_dir: Option<&str>) -> bool {
        let sanitized: String = session
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
            .collect();

        if sanitized.is_empty() {
            return false;
        }

        let cmd = if let Some(cwd) = working_dir {
            let cygwin_path = Self::windows_to_cygwin_path(cwd);
            format!("tmux new-session -d -s '{}' -c '{}'", sanitized, cygwin_path)
        } else {
            format!("tmux new-session -d -s '{}'", sanitized)
        };

        self.execute(&cmd).is_some()
    }

    fn is_available(&self) -> bool {
        self.itmux_path.is_some() && self.bash_path.is_some()
    }

    fn get_version(&self) -> Option<String> {
        self.execute("tmux -V").map(|s| s.trim().to_string())
    }

    fn get_active_pane(&self, session: &str) -> Option<String> {
        let escaped = Self::escape_session(session);
        let output = self.execute(&format!(
            "tmux display-message -t '{}' -p \"#{{pane_id}}\"",
            escaped
        ))?;
        let trimmed = output.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    }

    fn get_pane_cwd(&self, session: &str, pane_id: Option<&str>) -> Option<String> {
        let escaped = Self::escape_session(session);
        let target = if let Some(pid) = pane_id {
            format!("{}:{}", escaped, pid)
        } else {
            escaped
        };

        let output = self.execute(&format!(
            "tmux display-message -t '{}' -p \"#{{pane_current_path}}\"",
            target
        ))?;
        let trimmed = output.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    }
}

// ============================================================================
// Helper functions
// ============================================================================

/// Get git info for a directory
pub fn get_git_info(cwd: &str) -> Option<GitInfo> {
    let path = Path::new(cwd);
    if !path.exists() {
        return None;
    }

    // Check if inside git work tree
    let result = Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(path)
        .output()
        .ok()?;

    if !result.status.success() {
        return None;
    }

    let run = |args: &[&str]| -> Option<String> {
        Command::new("git")
            .args(args)
            .current_dir(path)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    };

    let branch = run(&["rev-parse", "--abbrev-ref", "HEAD"]);
    let remote = run(&["config", "--get", "remote.origin.url"]);

    let repo = remote.as_ref().and_then(|r| {
        r.split("github.com")
            .nth(1)
            .map(|s| s.trim_start_matches([':', '/']).replace(".git", ""))
    });

    Some(GitInfo {
        branch,
        remote,
        repo,
    })
}

/// Scan for all tmux sessions
pub fn scan_sessions() -> Vec<String> {
    create_executor().list_sessions()
}

/// List tmux sessions with details
pub fn list_sessions() -> Vec<TmuxSession> {
    let executor = create_executor();
    executor
        .list_sessions()
        .into_iter()
        .map(|name| TmuxSession {
            name,
            windows: 1, // Simplified
            attached: false,
        })
        .collect()
}

/// Send keys to tmux session
pub fn send_keys(target: &str, keys: &str, enter: bool) -> bool {
    let executor = create_executor();

    // Handle special keys
    match keys {
        "\x03" => return executor.send_special_key(target, "C-c"),
        "\x04" => return executor.send_special_key(target, "C-d"),
        "\n" | "\r\n" => return executor.send_special_key(target, "Enter"),
        _ => {}
    }

    // Handle text with newline at end
    if let Some(cmd) = keys.strip_suffix('\n') {
        if !cmd.is_empty() {
            executor.send_keys(target, cmd);
        }
        return executor.send_special_key(target, "Enter");
    }

    executor.send_keys(target, keys);

    if enter {
        executor.send_special_key(target, "Enter");
    }

    true
}

/// Resize tmux window
pub fn resize_window(session: &str, cols: usize, rows: usize) -> bool {
    create_executor().resize_window(session, cols, rows)
}

/// Create new tmux session
pub fn create_session(session_name: &str, working_dir: Option<&str>) -> bool {
    create_executor().create_session(session_name, working_dir)
}

/// Kill tmux session
pub fn kill_session(session_name: &str) -> bool {
    create_executor().kill_session(session_name)
}

/// Get active pane ID
pub fn get_active_pane(session: &str) -> Option<String> {
    create_executor().get_active_pane(session)
}

/// Get pane current working directory
pub fn get_pane_cwd(session: &str, pane_id: Option<&str>) -> Option<String> {
    create_executor().get_pane_cwd(session, pane_id)
}

/// List panes in session
pub fn list_panes(session: &str) -> Option<Vec<PaneData>> {
    create_executor().list_panes(session)
}

/// Capture pane content
pub fn capture_pane(session: &str) -> Option<String> {
    create_executor().capture_pane(session)
}

/// Capture pane by ID
pub fn capture_pane_by_id(session: &str, pane_id: &str) -> Option<String> {
    create_executor().capture_pane_by_id(session, pane_id)
}
