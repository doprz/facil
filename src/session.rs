use std::collections::HashMap;
use std::path::PathBuf;

use crate::config::model::Project;
use crate::error::Error;
use crate::tmux::Tmux;

#[derive(Debug, Clone)]
pub enum PlanStep {
    NewSession {
        window_name: String,
        root: Option<PathBuf>,
        tmux_options: Option<String>,
    },
    Host(String),
    NewWindow {
        window_name: String,
        root: Option<PathBuf>,
    },
    SplitPane {
        window_name: String,
        pane_index: usize,
        root: Option<PathBuf>,
    },
    SendKeys {
        window_name: String,
        pane_index: usize,
        command: String,
    },
    SelectLayout {
        window_name: String,
        layout: String,
    },
}

/// Build the ordered, symbolic list of steps for `project`, matching the spec's
/// build order: new-session, pre, create windows, split panes, pre_window + pane
/// commands, select-layout, post. (Attach happens outside the plan.)
pub fn build_plan(project: &Project) -> Vec<PlanStep> {
    let mut steps = Vec::new();

    let first_window = &project.windows[0];
    steps.push(PlanStep::NewSession {
        window_name: first_window.name.clone(),
        root: project.effective_root(first_window.panes.first()),
        tmux_options: project.tmux_options.clone(),
    });

    for cmd in &project.pre {
        steps.push(PlanStep::Host(cmd.clone()));
    }

    for window in project.windows.iter().skip(1) {
        steps.push(PlanStep::NewWindow {
            window_name: window.name.clone(),
            root: project.effective_root(window.panes.first()),
        });
    }

    for window in &project.windows {
        for (pi, pane) in window.panes.iter().enumerate().skip(1) {
            steps.push(PlanStep::SplitPane {
                window_name: window.name.clone(),
                pane_index: pi,
                root: project.effective_root(Some(pane)),
            });
        }
    }

    for window in &project.windows {
        for (pi, pane) in window.panes.iter().enumerate() {
            for cmd in &window.pre_window {
                steps.push(PlanStep::SendKeys {
                    window_name: window.name.clone(),
                    pane_index: pi,
                    command: cmd.clone(),
                });
            }
            for cmd in &pane.commands {
                steps.push(PlanStep::SendKeys {
                    window_name: window.name.clone(),
                    pane_index: pi,
                    command: cmd.clone(),
                });
            }
        }
    }

    for window in &project.windows {
        if let Some(layout) = &window.layout {
            steps.push(PlanStep::SelectLayout {
                window_name: window.name.clone(),
                layout: layout.clone(),
            });
        }
    }

    for cmd in &project.post {
        steps.push(PlanStep::Host(cmd.clone()));
    }

    steps
}

/// Render the plan as human-readable command lines, without executing anything.
/// Pane targets are shown symbolically (window.pane_index) since real pane ids
/// are only known once tmux actually creates them.
pub fn render(session: &str, steps: &[PlanStep]) -> String {
    let mut out = String::new();
    for step in steps {
        let line = match step {
            PlanStep::NewSession {
                window_name,
                root,
                tmux_options,
            } => {
                let extra = tmux_options
                    .as_deref()
                    .map(|o| format!(" {o}"))
                    .unwrap_or_default();
                format!(
                    "tmux new-session -d -s {session} -n {window_name}{}{extra}",
                    root_suffix(root)
                )
            }
            PlanStep::Host(cmd) => format!("(host) {cmd}"),
            PlanStep::NewWindow { window_name, root } => {
                format!(
                    "tmux new-window -t {session}: -n {window_name}{}",
                    root_suffix(root)
                )
            }
            PlanStep::SplitPane {
                window_name,
                pane_index,
                root,
            } => {
                format!(
                    "tmux split-window -t {session}:{window_name}{}",
                    root_suffix(root)
                ) + &format!("  # -> {session}:{window_name}.{pane_index}")
            }
            PlanStep::SendKeys {
                window_name,
                pane_index,
                command,
            } => {
                format!("tmux send-keys -t {session}:{window_name}.{pane_index} {command:?} Enter")
            }
            PlanStep::SelectLayout {
                window_name,
                layout,
            } => {
                format!("tmux select-layout -t {session}:{window_name} {layout}")
            }
        };
        out.push_str(&line);
        out.push('\n');
    }
    out.push_str(&format!("tmux attach -t {session}\n"));
    out
}

fn root_suffix(root: &Option<PathBuf>) -> String {
    match root {
        Some(p) => format!(" -c {}", p.display()),
        None => String::new(),
    }
}

/// Execute the plan against a live tmux, resolving real pane ids as panes are
/// created (never assuming a base-index of 0).
pub fn execute(session: &str, steps: &[PlanStep], tmux: &Tmux) -> Result<(), Error> {
    let mut pane_ids: HashMap<(String, usize), String> = HashMap::new();

    for step in steps {
        match step {
            PlanStep::NewSession {
                window_name,
                root,
                tmux_options,
            } => {
                let pane_id = tmux.new_session(
                    session,
                    window_name,
                    root.as_deref(),
                    tmux_options.as_deref(),
                )?;
                pane_ids.insert((window_name.clone(), 0), pane_id);
            }
            PlanStep::Host(cmd) => run_host_command(cmd)?,
            PlanStep::NewWindow { window_name, root } => {
                let pane_id = tmux.new_window(session, window_name, root.as_deref())?;
                pane_ids.insert((window_name.clone(), 0), pane_id);
            }
            PlanStep::SplitPane {
                window_name,
                pane_index,
                root,
            } => {
                let base = pane_ids
                    .get(&(window_name.clone(), 0))
                    .expect("window's first pane must exist before splitting");
                let pane_id = tmux.split_window(base, root.as_deref())?;
                pane_ids.insert((window_name.clone(), *pane_index), pane_id);
            }
            PlanStep::SendKeys {
                window_name,
                pane_index,
                command,
            } => {
                let target = pane_ids
                    .get(&(window_name.clone(), *pane_index))
                    .expect("pane must exist before sending keys");
                tmux.send_keys(target, command)?;
            }
            PlanStep::SelectLayout {
                window_name,
                layout,
            } => {
                let target = pane_ids
                    .get(&(window_name.clone(), 0))
                    .expect("window's first pane must exist before selecting layout");
                tmux.select_layout(target, layout)?;
            }
        }
    }

    Ok(())
}

fn run_host_command(cmd: &str) -> Result<(), Error> {
    let status = std::process::Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(Error::HostCommand {
            command: cmd.to_string(),
            status: status.code().unwrap_or(-1),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::model::{Pane, Window};

    fn project() -> Project {
        Project {
            name: "myproject".to_string(),
            root: Some("/tmp".to_string()),
            pre: vec!["echo pre".to_string()],
            post: vec!["echo post".to_string()],
            tmux_options: None,
            socket_name: None,
            windows: vec![
                Window {
                    name: "editor".to_string(),
                    layout: Some("main-vertical".to_string()),
                    pre_window: vec![],
                    panes: vec![
                        Pane {
                            commands: vec!["nvim .".to_string()],
                            root: None,
                        },
                        Pane {
                            commands: vec!["cargo watch -x test".to_string()],
                            root: None,
                        },
                    ],
                },
                Window {
                    name: "server".to_string(),
                    layout: None,
                    pre_window: vec!["source .env".to_string()],
                    panes: vec![Pane {
                        commands: vec!["docker compose up".to_string()],
                        root: None,
                    }],
                },
            ],
        }
    }

    #[test]
    fn build_order_matches_spec() {
        let steps = build_plan(&project());
        let kinds: Vec<&str> = steps
            .iter()
            .map(|s| match s {
                PlanStep::NewSession { .. } => "new-session",
                PlanStep::Host(_) => "host",
                PlanStep::NewWindow { .. } => "new-window",
                PlanStep::SplitPane { .. } => "split-pane",
                PlanStep::SendKeys { .. } => "send-keys",
                PlanStep::SelectLayout { .. } => "select-layout",
            })
            .collect();

        assert_eq!(
            kinds,
            vec![
                "new-session",
                "host",          // pre
                "new-window",    // server
                "split-pane",    // editor pane 1
                "send-keys",     // editor.0 nvim
                "send-keys",     // editor.1 cargo watch
                "send-keys",     // server pre_window
                "send-keys",     // server pane command
                "select-layout", // editor
                "host",          // post
            ]
        );
    }

    #[test]
    fn first_pane_of_first_window_is_never_split() {
        let steps = build_plan(&project());
        for step in &steps {
            if let PlanStep::SplitPane {
                window_name,
                pane_index,
                ..
            } = step
            {
                assert!(!(window_name == "editor" && *pane_index == 0));
            }
        }
    }
}
