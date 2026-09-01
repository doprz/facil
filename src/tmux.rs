use std::collections::HashMap;
use std::path::Path;
use std::process::{Command, Output};

use crate::error::TmuxError;

pub struct Tmux {
    socket_name: Option<String>,
    verbose: u8,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionInfo {
    pub name: String,
    pub windows: usize,
    pub panes: usize,
    pub attached: bool,
    pub created: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WindowInfo {
    pub name: String,
    pub layout: String,
}

fn parse_session_line(line: &str) -> Option<SessionInfo> {
    let mut parts = line.splitn(4, '\t');
    let name = parts.next()?.to_string();
    let windows: usize = parts.next()?.parse().ok()?;
    let attached: u32 = parts.next()?.parse().ok()?;
    let created: i64 = parts.next()?.parse().ok()?;
    Some(SessionInfo {
        name,
        windows,
        panes: 0,
        attached: attached > 0,
        created,
    })
}

fn count_panes_by_session(raw: &str) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    for line in raw.lines().filter(|l| !l.is_empty()) {
        *counts.entry(line.to_string()).or_insert(0) += 1;
    }
    counts
}

/// Raw `tmux -V` output (e.g. "tmux 3.7b"), no socket/session context needed.
pub fn version() -> Result<String, TmuxError> {
    let output = Command::new("tmux").arg("-V").output().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            TmuxError::NotFound
        } else {
            TmuxError::Spawn(e)
        }
    })?;
    if !output.status.success() {
        return Err(TmuxError::CommandFailed {
            status: output.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// The name of the tmux session this process is currently running inside, if
/// any. `None` outside tmux or if the query fails for any reason - this is a
/// convenience fallback for name resolution, never a hard requirement.
pub fn current_session_name() -> Option<String> {
    std::env::var("TMUX").ok()?;
    let output = Command::new("tmux")
        .args(["display-message", "-p", "#S"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if name.is_empty() { None } else { Some(name) }
}

impl Tmux {
    pub fn new(socket_name: Option<String>, verbose: u8) -> Self {
        Self {
            socket_name,
            verbose,
        }
    }

    fn command(&self) -> Command {
        let mut cmd = Command::new("tmux");
        if let Some(socket) = &self.socket_name {
            cmd.args(["-L", socket]);
        }
        cmd
    }

    fn run(&self, args: &[&str]) -> Result<Output, TmuxError> {
        if self.verbose >= 1 {
            eprintln!("+ tmux {}", args.join(" "));
        }
        let output = self.command().args(args).output().map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                TmuxError::NotFound
            } else {
                TmuxError::Spawn(e)
            }
        })?;
        if self.verbose >= 2 {
            eprint!("{}", String::from_utf8_lossy(&output.stdout));
            eprint!("{}", String::from_utf8_lossy(&output.stderr));
        }
        Ok(output)
    }

    fn run_ok(&self, args: &[&str]) -> Result<(), TmuxError> {
        let output = self.run(args)?;
        if output.status.success() {
            Ok(())
        } else {
            Err(TmuxError::CommandFailed {
                status: output.status.code().unwrap_or(-1),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            })
        }
    }

    fn run_capture_pane_id(&self, mut args: Vec<&str>) -> Result<String, TmuxError> {
        args.push("-P");
        args.push("-F");
        args.push("#{pane_id}");
        let output = self.run(&args)?;
        if !output.status.success() {
            return Err(TmuxError::CommandFailed {
                status: output.status.code().unwrap_or(-1),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            });
        }
        let pane_id = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if pane_id.is_empty() {
            Err(TmuxError::NoPaneId(args.join(" ")))
        } else {
            Ok(pane_id)
        }
    }

    pub fn has_session(&self, session: &str) -> Result<bool, TmuxError> {
        let output = self.run(&["has-session", "-t", session])?;
        Ok(output.status.success())
    }

    /// All sessions on this socket. `Ok(vec![])` (not an error) when there's no
    /// tmux server running on the socket at all - same "nothing there" convention
    /// as `has_session`.
    pub fn list_sessions(&self) -> Result<Vec<SessionInfo>, TmuxError> {
        let output = self.run(&[
            "list-sessions",
            "-F",
            "#{session_name}\t#{session_windows}\t#{session_attached}\t#{session_created}",
        ])?;
        if !output.status.success() {
            return Ok(vec![]);
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut sessions: Vec<SessionInfo> =
            stdout.lines().filter_map(parse_session_line).collect();

        let pane_output = self.run(&["list-panes", "-a", "-F", "#{session_name}"])?;
        if pane_output.status.success() {
            let counts = count_panes_by_session(&String::from_utf8_lossy(&pane_output.stdout));
            for session in &mut sessions {
                session.panes = counts.get(&session.name).copied().unwrap_or(0);
            }
        }

        Ok(sessions)
    }

    /// This session's windows, in index order, or `SessionNotFound` if it doesn't exist.
    pub fn list_windows(&self, session: &str) -> Result<Vec<WindowInfo>, TmuxError> {
        if !self.has_session(session)? {
            return Err(TmuxError::SessionNotFound(session.to_string()));
        }
        let output = self.run(&[
            "list-windows",
            "-t",
            session,
            "-F",
            "#{window_name}\t#{window_layout}",
        ])?;
        if !output.status.success() {
            return Err(TmuxError::CommandFailed {
                status: output.status.code().unwrap_or(-1),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            });
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(stdout
            .lines()
            .filter_map(|line| {
                let (name, layout) = line.split_once('\t')?;
                Some(WindowInfo {
                    name: name.to_string(),
                    layout: layout.to_string(),
                })
            })
            .collect())
    }

    /// Every pane's current working directory in `target` (a window), in pane-index order.
    pub fn list_panes(&self, target: &str) -> Result<Vec<String>, TmuxError> {
        let output = self.run(&["list-panes", "-t", target, "-F", "#{pane_current_path}"])?;
        if !output.status.success() {
            return Err(TmuxError::CommandFailed {
                status: output.status.code().unwrap_or(-1),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            });
        }
        Ok(String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(str::to_string)
            .collect())
    }

    pub fn new_session(
        &self,
        session: &str,
        window_name: Option<&str>,
        root: Option<&Path>,
        extra_options: Option<&str>,
    ) -> Result<String, TmuxError> {
        let root_str = root.map(|p| p.display().to_string());
        let mut args = vec!["new-session", "-d", "-s", session];
        if let Some(name) = window_name {
            args.push("-n");
            args.push(name);
        }
        if let Some(r) = &root_str {
            args.push("-c");
            args.push(r);
        }
        let extra: Vec<&str> = extra_options
            .map(|s| s.split_whitespace().collect())
            .unwrap_or_default();
        args.extend(extra);
        self.run_capture_pane_id(args)
    }

    pub fn new_window(
        &self,
        session: &str,
        window_name: Option<&str>,
        root: Option<&Path>,
    ) -> Result<String, TmuxError> {
        let target = format!("{session}:");
        let root_str = root.map(|p| p.display().to_string());
        let mut args = vec!["new-window", "-t", target.as_str()];
        if let Some(name) = window_name {
            args.push("-n");
            args.push(name);
        }
        if let Some(r) = &root_str {
            args.push("-c");
            args.push(r);
        }
        self.run_capture_pane_id(args)
    }

    pub fn split_window(
        &self,
        target_pane: &str,
        root: Option<&Path>,
    ) -> Result<String, TmuxError> {
        let root_str = root.map(|p| p.display().to_string());
        let mut args = vec!["split-window", "-t", target_pane];
        if let Some(r) = &root_str {
            args.push("-c");
            args.push(r);
        }
        self.run_capture_pane_id(args)
    }

    pub fn send_keys(&self, target_pane: &str, command: &str) -> Result<(), TmuxError> {
        self.run_ok(&["send-keys", "-t", target_pane, command, "Enter"])
    }

    pub fn select_layout(&self, target_pane: &str, layout: &str) -> Result<(), TmuxError> {
        self.run_ok(&["select-layout", "-t", target_pane, layout])
    }

    /// Make the window containing `target_pane` the session's active window.
    /// Targeting via a pane id (rather than a window name/index) means this
    /// works regardless of the user's `base-index` and even for unnamed windows.
    pub fn select_window(&self, target_pane: &str) -> Result<(), TmuxError> {
        self.run_ok(&["select-window", "-t", target_pane])
    }

    pub fn kill_session(&self, session: &str) -> Result<(), TmuxError> {
        self.run_ok(&["kill-session", "-t", session])
    }

    /// Replace the current process with `tmux attach`, or `switch-client` if already
    /// inside a tmux client (nested attach otherwise fails/misbehaves).
    pub fn attach_or_switch(&self, session: &str) -> Result<(), TmuxError> {
        use std::os::unix::process::CommandExt;

        let subcmd = if std::env::var("TMUX").is_ok() {
            "switch-client"
        } else {
            "attach-session"
        };
        let err = self.command().args([subcmd, "-t", session]).exec();
        if err.kind() == std::io::ErrorKind::NotFound {
            Err(TmuxError::NotFound)
        } else {
            Err(TmuxError::Spawn(err))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_session_name_outside_tmux_is_none() {
        // Only assert when this test binary itself isn't running inside tmux -
        // mutating $TMUX here would race with other tests in this same process.
        if std::env::var("TMUX").is_err() {
            assert_eq!(current_session_name(), None);
        }
    }

    #[test]
    fn parses_session_line() {
        let info = parse_session_line("myproject\t2\t1\t1700000000").unwrap();
        assert_eq!(
            info,
            SessionInfo {
                name: "myproject".to_string(),
                windows: 2,
                panes: 0,
                attached: true,
                created: 1700000000
            }
        );
    }

    #[test]
    fn parses_unattached_session() {
        let info = parse_session_line("myproject\t1\t0\t1700000000").unwrap();
        assert!(!info.attached);
    }

    #[test]
    fn rejects_malformed_session_line() {
        assert!(parse_session_line("not enough fields").is_none());
    }

    #[test]
    fn counts_panes_by_session() {
        let raw = "editor\neditor\nserver\n";
        let counts = count_panes_by_session(raw);
        assert_eq!(counts.get("editor"), Some(&2));
        assert_eq!(counts.get("server"), Some(&1));
    }
}
