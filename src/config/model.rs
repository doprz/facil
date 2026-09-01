use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
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
    /// A window's explicit name, or its 1-based position (as a string, e.g. `"2"`),
    /// selected as the active window right before attaching.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attach_window: Option<String>,
    #[serde(default)]
    pub windows: Vec<Window>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Window {
    /// The tmux window name. `None` means no `-n` is passed at all, so tmux
    /// picks its own default name/number rather than facil inventing one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layout: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pre_window: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub panes: Vec<Pane>,
}

#[derive(Debug, Deserialize, Serialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Pane {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub commands: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root: Option<String>,
}

impl Project {
    /// Effective, tilde-expanded root for a pane in `window`: pane override,
    /// else window override, else project root.
    pub fn effective_root(&self, window: &Window, pane: Option<&Pane>) -> Option<PathBuf> {
        let raw = pane
            .and_then(|p| p.root.as_deref())
            .or(window.root.as_deref())
            .or(self.root.as_deref())?;
        Some(expand_tilde(raw))
    }

    /// Resolve `attach_window` (an explicit window name or 1-based position
    /// number) to a 0-based index into `windows`. `Ok(None)` if unset. `Err`
    /// names what was tried when it doesn't match any window.
    pub fn resolve_attach_window(&self) -> Result<Option<usize>, String> {
        let Some(target) = &self.attach_window else {
            return Ok(None);
        };

        if let Some(idx) = self
            .windows
            .iter()
            .position(|w| w.name.as_deref() == Some(target.as_str()))
        {
            return Ok(Some(idx));
        }

        if let Ok(n) = target.parse::<usize>()
            && n >= 1
            && n <= self.windows.len()
        {
            return Ok(Some(n - 1));
        }

        Err(format!(
            "no window named `{target}` and not a valid window number (1-{})",
            self.windows.len()
        ))
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

    fn window(root: Option<&str>) -> Window {
        Window {
            name: None,
            layout: None,
            root: root.map(str::to_string),
            pre_window: vec![],
            panes: vec![],
        }
    }

    fn pane(root: Option<&str>) -> Pane {
        Pane {
            commands: vec![],
            root: root.map(str::to_string),
        }
    }

    fn project(root: Option<&str>) -> Project {
        Project {
            name: "p".to_string(),
            root: root.map(str::to_string),
            pre: vec![],
            post: vec![],
            tmux_options: None,
            socket_name: None,
            attach_window: None,
            windows: vec![],
        }
    }

    #[test]
    fn pane_root_wins_over_window_and_project() {
        let p = project(Some("/project"));
        let w = window(Some("/window"));
        let pn = pane(Some("/pane"));
        assert_eq!(
            p.effective_root(&w, Some(&pn)),
            Some(PathBuf::from("/pane"))
        );
    }

    #[test]
    fn window_root_wins_over_project_when_pane_unset() {
        let p = project(Some("/project"));
        let w = window(Some("/window"));
        assert_eq!(p.effective_root(&w, None), Some(PathBuf::from("/window")));
    }

    #[test]
    fn falls_back_to_project_root() {
        let p = project(Some("/project"));
        let w = window(None);
        assert_eq!(p.effective_root(&w, None), Some(PathBuf::from("/project")));
    }
}
