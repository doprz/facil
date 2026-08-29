use std::path::PathBuf;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Project {
    pub name: String,
    pub root: Option<String>,
    #[serde(default)]
    pub pre: Vec<String>,
    #[serde(default)]
    pub post: Vec<String>,
    pub tmux_options: Option<String>,
    pub socket_name: Option<String>,
    #[serde(default)]
    pub windows: Vec<Window>,
}

#[derive(Debug, Deserialize)]
pub struct Window {
    pub name: String,
    pub layout: Option<String>,
    #[serde(default)]
    pub pre_window: Vec<String>,
    #[serde(default)]
    pub panes: Vec<Pane>,
}

#[derive(Debug, Deserialize, Default)]
pub struct Pane {
    #[serde(default)]
    pub commands: Vec<String>,
    pub root: Option<String>,
}

impl Project {
    /// Effective, tilde-expanded root for a given pane (pane override, else project root).
    pub fn effective_root(&self, pane: Option<&Pane>) -> Option<PathBuf> {
        let raw = pane.and_then(|p| p.root.as_deref()).or(self.root.as_deref())?;
        Some(expand_tilde(raw))
    }
}

pub fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(rest);
        }
    } else if path == "~"
        && let Ok(home) = std::env::var("HOME")
    {
        return PathBuf::from(home);
    }
    PathBuf::from(path)
}
