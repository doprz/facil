use std::path::Path;
use std::process::{Command, Output};

use crate::error::TmuxError;

pub struct Tmux {
    socket_name: Option<String>,
    verbose: u8,
}

impl Tmux {
    pub fn new(socket_name: Option<String>, verbose: u8) -> Self {
        Self { socket_name, verbose }
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

    pub fn new_session(
        &self,
        session: &str,
        window_name: &str,
        root: Option<&Path>,
        extra_options: Option<&str>,
    ) -> Result<String, TmuxError> {
        let root_str = root.map(|p| p.display().to_string());
        let mut args = vec!["new-session", "-d", "-s", session, "-n", window_name];
        if let Some(r) = &root_str {
            args.push("-c");
            args.push(r);
        }
        let extra: Vec<&str> = extra_options.map(|s| s.split_whitespace().collect()).unwrap_or_default();
        args.extend(extra);
        self.run_capture_pane_id(args)
    }

    pub fn new_window(&self, session: &str, window_name: &str, root: Option<&Path>) -> Result<String, TmuxError> {
        let target = format!("{session}:");
        let root_str = root.map(|p| p.display().to_string());
        let mut args = vec!["new-window", "-t", target.as_str(), "-n", window_name];
        if let Some(r) = &root_str {
            args.push("-c");
            args.push(r);
        }
        self.run_capture_pane_id(args)
    }

    pub fn split_window(&self, target_pane: &str, root: Option<&Path>) -> Result<String, TmuxError> {
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

    pub fn kill_session(&self, session: &str) -> Result<(), TmuxError> {
        self.run_ok(&["kill-session", "-t", session])
    }

    /// Replace the current process with `tmux attach`, or `switch-client` if already
    /// inside a tmux client (nested attach otherwise fails/misbehaves).
    pub fn attach_or_switch(&self, session: &str) -> Result<(), TmuxError> {
        use std::os::unix::process::CommandExt;

        let subcmd = if std::env::var("TMUX").is_ok() { "switch-client" } else { "attach-session" };
        let err = self.command().args([subcmd, "-t", session]).exec();
        if err.kind() == std::io::ErrorKind::NotFound {
            Err(TmuxError::NotFound)
        } else {
            Err(TmuxError::Spawn(err))
        }
    }
}
