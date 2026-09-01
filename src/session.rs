use std::collections::HashMap;
use std::path::PathBuf;

use crate::config::model::Project;
use crate::error::Error;
use crate::tmux::Tmux;

#[derive(Debug, Clone)]
pub enum PlanStep {
    NewSession {
        window_index: usize,
        window_name: Option<String>,
        root: Option<PathBuf>,
        tmux_options: Option<String>,
    },
    Host(String),
    NewWindow {
        window_index: usize,
        window_name: Option<String>,
        root: Option<PathBuf>,
    },
    SplitPane {
        window_index: usize,
        window_name: Option<String>,
        pane_index: usize,
        root: Option<PathBuf>,
    },
    SendKeys {
        window_index: usize,
        window_name: Option<String>,
        pane_index: usize,
        command: String,
    },
    SelectLayout {
        window_index: usize,
        window_name: Option<String>,
        layout: String,
    },
    SelectWindow {
        window_index: usize,
        window_name: Option<String>,
    },
}

/// Build the ordered, symbolic list of steps for `project`, matching the spec's
/// build order: new-session, pre, create windows, split panes, pre_window + pane
/// commands, select-layout, select the attach_window, post. (Attach itself
/// happens outside the plan.) `window_name` travels alongside `window_index`
/// purely so `render()` can show a friendly label - execution only ever keys
/// off `window_index`, since a window may have no name at all.
pub fn build_plan(project: &Project) -> Vec<PlanStep> {
    let mut steps = Vec::new();

    let first_window = &project.windows[0];
    steps.push(PlanStep::NewSession {
        window_index: 0,
        window_name: first_window.name.clone(),
        root: project.effective_root(first_window, first_window.panes.first()),
        tmux_options: project.tmux_options.clone(),
    });

    for cmd in &project.pre {
        steps.push(PlanStep::Host(cmd.clone()));
    }

    for (wi, window) in project.windows.iter().enumerate().skip(1) {
        steps.push(PlanStep::NewWindow {
            window_index: wi,
            window_name: window.name.clone(),
            root: project.effective_root(window, window.panes.first()),
        });
    }

    for (wi, window) in project.windows.iter().enumerate() {
        for (pi, pane) in window.panes.iter().enumerate().skip(1) {
            steps.push(PlanStep::SplitPane {
                window_index: wi,
                window_name: window.name.clone(),
                pane_index: pi,
                root: project.effective_root(window, Some(pane)),
            });
        }
    }

    for (wi, window) in project.windows.iter().enumerate() {
        for (pi, pane) in window.panes.iter().enumerate() {
            for cmd in &window.pre_window {
                steps.push(PlanStep::SendKeys {
                    window_index: wi,
                    window_name: window.name.clone(),
                    pane_index: pi,
                    command: cmd.clone(),
                });
            }
            for cmd in &pane.commands {
                steps.push(PlanStep::SendKeys {
                    window_index: wi,
                    window_name: window.name.clone(),
                    pane_index: pi,
                    command: cmd.clone(),
                });
            }
        }
    }

    for (wi, window) in project.windows.iter().enumerate() {
        if let Some(layout) = &window.layout {
            steps.push(PlanStep::SelectLayout {
                window_index: wi,
                window_name: window.name.clone(),
                layout: layout.clone(),
            });
        }
    }

    // `resolve_attach_window` only errs when the value doesn't resolve to a
    // real window; validation already rejects that before a plan is ever
    // built, so an Err here is unreachable in practice - ignore it rather
    // than panic.
    if let Ok(Some(idx)) = project.resolve_attach_window() {
        steps.push(PlanStep::SelectWindow {
            window_index: idx,
            window_name: project.windows[idx].name.clone(),
        });
    }

    for cmd in &project.post {
        steps.push(PlanStep::Host(cmd.clone()));
    }

    steps
}

/// A window's name if it has one, else a readable placeholder - debug output
/// is symbolic already (real pane ids don't exist until execution), so an
/// honest "no name was given" placeholder is more useful than guessing one.
fn window_label(index: usize, name: &Option<String>) -> String {
    match name {
        Some(n) => n.clone(),
        None => format!("<window {}>", index + 1),
    }
}

/// Render the plan as human-readable command lines, without executing anything.
/// Pane/window targets are shown symbolically since real pane ids are only
/// known once tmux actually creates them.
pub fn render(session: &str, steps: &[PlanStep]) -> String {
    let mut out = String::new();
    for step in steps {
        let line = match step {
            PlanStep::NewSession {
                window_index,
                window_name,
                root,
                tmux_options,
            } => {
                let extra = tmux_options
                    .as_deref()
                    .map(|o| format!(" {o}"))
                    .unwrap_or_default();
                let name_flag = window_name
                    .as_deref()
                    .map(|n| format!(" -n {n}"))
                    .unwrap_or_default();
                format!(
                    "tmux new-session -d -s {session}{name_flag}{}{extra}  # -> {session}:{}",
                    root_suffix(root),
                    window_label(*window_index, window_name)
                )
            }
            PlanStep::Host(cmd) => format!("(host) {cmd}"),
            PlanStep::NewWindow {
                window_index,
                window_name,
                root,
            } => {
                let name_flag = window_name
                    .as_deref()
                    .map(|n| format!(" -n {n}"))
                    .unwrap_or_default();
                format!(
                    "tmux new-window -t {session}:{name_flag}{}  # -> {session}:{}",
                    root_suffix(root),
                    window_label(*window_index, window_name)
                )
            }
            PlanStep::SplitPane {
                window_index,
                window_name,
                pane_index,
                root,
            } => {
                let label = window_label(*window_index, window_name);
                format!(
                    "tmux split-window -t {session}:{label}{}",
                    root_suffix(root)
                ) + &format!("  # -> {session}:{label}.{pane_index}")
            }
            PlanStep::SendKeys {
                window_index,
                window_name,
                pane_index,
                command,
            } => {
                let label = window_label(*window_index, window_name);
                format!("tmux send-keys -t {session}:{label}.{pane_index} {command:?} Enter")
            }
            PlanStep::SelectLayout {
                window_index,
                window_name,
                layout,
            } => {
                let label = window_label(*window_index, window_name);
                format!("tmux select-layout -t {session}:{label} {layout}")
            }
            PlanStep::SelectWindow {
                window_index,
                window_name,
            } => {
                let label = window_label(*window_index, window_name);
                format!("tmux select-window -t {session}:{label}")
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
/// created (never assuming a base-index of 0, and never passing a window name
/// to tmux unless the config actually gave it one).
pub fn execute(session: &str, steps: &[PlanStep], tmux: &Tmux) -> Result<(), Error> {
    let mut pane_ids: HashMap<(usize, usize), String> = HashMap::new();

    for step in steps {
        match step {
            PlanStep::NewSession {
                window_index,
                window_name,
                root,
                tmux_options,
            } => {
                let pane_id = tmux.new_session(
                    session,
                    window_name.as_deref(),
                    root.as_deref(),
                    tmux_options.as_deref(),
                )?;
                pane_ids.insert((*window_index, 0), pane_id);
            }
            PlanStep::Host(cmd) => run_host_command(cmd)?,
            PlanStep::NewWindow {
                window_index,
                window_name,
                root,
            } => {
                let pane_id = tmux.new_window(session, window_name.as_deref(), root.as_deref())?;
                pane_ids.insert((*window_index, 0), pane_id);
            }
            PlanStep::SplitPane {
                window_index,
                pane_index,
                root,
                ..
            } => {
                let base = pane_ids
                    .get(&(*window_index, 0))
                    .expect("window's first pane must exist before splitting");
                let pane_id = tmux.split_window(base, root.as_deref())?;
                pane_ids.insert((*window_index, *pane_index), pane_id);
            }
            PlanStep::SendKeys {
                window_index,
                pane_index,
                command,
                ..
            } => {
                let target = pane_ids
                    .get(&(*window_index, *pane_index))
                    .expect("pane must exist before sending keys");
                tmux.send_keys(target, command)?;
            }
            PlanStep::SelectLayout {
                window_index,
                layout,
                ..
            } => {
                let target = pane_ids
                    .get(&(*window_index, 0))
                    .expect("window's first pane must exist before selecting layout");
                tmux.select_layout(target, layout)?;
            }
            PlanStep::SelectWindow { window_index, .. } => {
                let target = pane_ids
                    .get(&(*window_index, 0))
                    .expect("window's first pane must exist before selecting it");
                tmux.select_window(target)?;
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
            attach_window: None,
            windows: vec![
                Window {
                    name: Some("editor".to_string()),
                    layout: Some("main-vertical".to_string()),
                    root: None,
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
                    name: Some("server".to_string()),
                    layout: None,
                    root: None,
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
                PlanStep::SelectWindow { .. } => "select-window",
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
                window_index,
                pane_index,
                ..
            } = step
            {
                assert!(!(*window_index == 0 && *pane_index == 0));
            }
        }
    }

    #[test]
    fn unnamed_window_gets_no_name_flag() {
        let mut p = project();
        p.windows[0].name = None;
        let steps = build_plan(&p);
        let rendered = render(&p.name, &steps);
        let first_line = rendered.lines().next().unwrap();
        assert!(first_line.contains("new-session -d -s myproject -c"));
        assert!(!first_line.contains(" -n "));
        // the still-named second window is unaffected
        assert!(rendered.contains("new-window -t myproject: -n server"));
    }

    #[test]
    fn attach_window_appends_select_window_step() {
        let mut p = project();
        p.attach_window = Some("server".to_string());
        let steps = build_plan(&p);
        let last_tmux_step = steps
            .iter()
            .rev()
            .find(|s| !matches!(s, PlanStep::Host(_)))
            .unwrap();
        assert!(matches!(
            last_tmux_step,
            PlanStep::SelectWindow {
                window_index: 1,
                ..
            }
        ));
    }

    #[test]
    fn no_attach_window_means_no_select_window_step() {
        let steps = build_plan(&project());
        assert!(
            !steps
                .iter()
                .any(|s| matches!(s, PlanStep::SelectWindow { .. }))
        );
    }
}
