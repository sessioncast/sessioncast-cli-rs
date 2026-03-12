mod executor;

pub use executor::*;

use serde::{Deserialize, Serialize};

/// Tmux pane data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaneData {
    pub id: String,
    pub index: usize,
    pub width: usize,
    pub height: usize,
    pub top: usize,
    pub left: usize,
    pub active: bool,
    pub title: String,
}

/// Tmux session info
#[derive(Debug, Clone)]
pub struct TmuxSession {
    pub name: String,
    pub windows: usize,
    pub attached: bool,
}

/// Git info for a directory
#[derive(Debug, Clone)]
pub struct GitInfo {
    pub branch: Option<String>,
    pub remote: Option<String>,
    pub repo: Option<String>,
}
