use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub struct Project {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pre: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub post: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tmux_options: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub socket_name: Option<String>,
    #[serde(default)]
    pub windows: Vec<Window>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Window {
    #[serde(default)]
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layout: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pre_window: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub panes: Vec<Pane>,
}

#[derive(Debug, Deserialize, Serialize, Default)]
pub struct Pane {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub commands: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root: Option<String>,
}

impl Project {
    /// Effective, tilde-expanded root for a given pane (pane override, else project root).
    pub fn effective_root(&self, pane: Option<&Pane>) -> Option<PathBuf> {
        let raw = pane
            .and_then(|p| p.root.as_deref())
            .or(self.root.as_deref())?;
        Some(expand_tilde(raw))
    }

    /// Assign `window1`, `window2`, ... to any window left unnamed in the TOML,
    /// by position. Called once right after parsing so validation, planning, and
    /// rendering never see a blank window name.
    pub fn fill_default_window_names(&mut self) {
        for (i, window) in self.windows.iter_mut().enumerate() {
            if window.name.trim().is_empty() {
                window.name = format!("window{}", i + 1);
            }
        }
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

/// Inverse of `expand_tilde`: rewrite a path back to `~/...` form when it's
/// under `$HOME`, so generated configs read the way a hand-written one would.
pub fn contract_home(path: &Path) -> String {
    if let Ok(home) = std::env::var("HOME")
        && let Ok(rest) = path.strip_prefix(&home)
    {
        return if rest.as_os_str().is_empty() {
            "~".to_string()
        } else {
            format!("~/{}", rest.display())
        };
    }
    path.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fills_only_blank_names() {
        let mut project = Project {
            name: "p".to_string(),
            root: None,
            pre: vec![],
            post: vec![],
            tmux_options: None,
            socket_name: None,
            windows: vec![
                Window {
                    name: "".to_string(),
                    layout: None,
                    pre_window: vec![],
                    panes: vec![],
                },
                Window {
                    name: "custom".to_string(),
                    layout: None,
                    pre_window: vec![],
                    panes: vec![],
                },
                Window {
                    name: "  ".to_string(),
                    layout: None,
                    pre_window: vec![],
                    panes: vec![],
                },
            ],
        };
        project.fill_default_window_names();
        assert_eq!(project.windows[0].name, "window1");
        assert_eq!(project.windows[1].name, "custom");
        assert_eq!(project.windows[2].name, "window3");
    }

    #[test]
    fn contract_home_roundtrips_expand_tilde() {
        let home = std::env::var("HOME").unwrap();
        let expanded = expand_tilde("~/code/myproject");
        assert_eq!(expanded, PathBuf::from(&home).join("code/myproject"));
        assert_eq!(contract_home(&expanded), "~/code/myproject");
    }

    #[test]
    fn contract_home_leaves_unrelated_paths_alone() {
        assert_eq!(contract_home(Path::new("/etc/passwd")), "/etc/passwd");
    }
}
