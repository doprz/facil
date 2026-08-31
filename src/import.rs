use serde_yaml_ng::{Mapping, Value};

use crate::config::model::{Pane, Project, Window};
use crate::error::ConfigError;

const KNOWN_TOP_LEVEL_KEYS: &[&str] = &[
    "name",
    "root",
    "project_root",
    "socket_name",
    "tmux_options",
    "pre",
    "post",
    "pre_window",
    "windows",
    "project_hooks",
];
const KNOWN_WINDOW_KEYS: &[&str] = &["layout", "root", "panes", "pre_window", "pre"];

/// Best-effort convert a tmuxinator YAML config to a facil `Project`. Hard-errors
/// only when the file can't be meaningfully converted at all (bad YAML, missing
/// `name`, `windows` not a list); anywhere else an unrecognized or unsupported
/// shape is skipped with a plain-English note rather than failing the whole
/// import, since partial success beats none for a review-before-use conversion.
pub fn convert(yaml: &str) -> Result<(Project, Vec<String>), ConfigError> {
    let value: Value = serde_yaml_ng::from_str(yaml)?;
    let root = value
        .as_mapping()
        .ok_or_else(|| ConfigError::ImportField("top-level YAML must be a mapping".to_string()))?;

    let mut warnings = Vec::new();

    let name = root
        .get("name")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| ConfigError::ImportField("missing top-level `name`".to_string()))?;

    let root_dir = root
        .get("root")
        .or_else(|| root.get("project_root"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let socket_name = root
        .get("socket_name")
        .and_then(Value::as_str)
        .map(str::to_string);
    let tmux_options = root
        .get("tmux_options")
        .and_then(Value::as_str)
        .map(str::to_string);

    let mut pre = as_str_list(root.get("pre"));
    let mut post = as_str_list(root.get("post"));

    if let Some(hooks) = root.get("project_hooks").and_then(Value::as_mapping) {
        if let Some(v) = hooks.get("on_project_start") {
            pre.extend(as_str_list(Some(v)));
            warnings.push(
                "project_hooks.on_project_start has no direct facil equivalent; appended to `pre`"
                    .to_string(),
            );
        }
        for key in ["on_project_exit", "on_project_stop"] {
            if let Some(v) = hooks.get(key) {
                post.extend(as_str_list(Some(v)));
                warnings.push(format!(
                    "project_hooks.{key} has no direct facil equivalent; appended to `post`"
                ));
            }
        }
        for key in ["on_project_first_start", "on_project_restart"] {
            if hooks.get(key).is_some() {
                warnings.push(format!(
                    "project_hooks.{key} is not supported by facil and was skipped"
                ));
            }
        }
    }

    if !pre.is_empty() {
        warnings.push(
            "facil's `pre` runs after the tmux session is created; tmuxinator's `pre` runs before \
             any tmux state exists - review timing-sensitive commands"
                .to_string(),
        );
    }

    let top_level_pre_window = as_str_list(root.get("pre_window"));

    for key in root.keys() {
        if let Some(key_str) = key.as_str()
            && !KNOWN_TOP_LEVEL_KEYS.contains(&key_str)
        {
            warnings.push(format!(
                "top-level `{key_str}` is not supported by facil and was skipped"
            ));
        }
    }

    let windows_value = root
        .get("windows")
        .ok_or_else(|| ConfigError::ImportField("missing top-level `windows`".to_string()))?;
    let windows_seq = windows_value
        .as_sequence()
        .ok_or_else(|| ConfigError::ImportField("`windows` must be a list".to_string()))?;

    let mut windows = Vec::with_capacity(windows_seq.len());
    for (i, entry) in windows_seq.iter().enumerate() {
        let (window, mut win_warnings) = convert_window(entry, &top_level_pre_window, i)?;
        warnings.append(&mut win_warnings);
        windows.push(window);
    }

    let project = Project {
        name,
        root: root_dir,
        pre,
        post,
        tmux_options,
        socket_name,
        windows,
    };
    Ok((project, warnings))
}

fn convert_window(
    entry: &Value,
    top_level_pre_window: &[String],
    index: usize,
) -> Result<(Window, Vec<String>), ConfigError> {
    let mapping = entry.as_mapping().ok_or_else(|| {
        ConfigError::ImportField(format!("windows[{index}] must be a single-key mapping"))
    })?;
    let (key, value) = mapping
        .iter()
        .next()
        .ok_or_else(|| ConfigError::ImportField(format!("windows[{index}] is empty")))?;
    let name = key.as_str().unwrap_or_default().to_string();
    let mut warnings = Vec::new();

    let window = match value {
        Value::String(cmd) => Window {
            name,
            layout: None,
            pre_window: top_level_pre_window.to_vec(),
            panes: vec![Pane {
                commands: vec![cmd.clone()],
                root: None,
            }],
        },
        Value::Null => Window {
            name,
            layout: None,
            pre_window: top_level_pre_window.to_vec(),
            panes: vec![Pane::default()],
        },
        Value::Mapping(m) => convert_window_mapping(name, m, top_level_pre_window, &mut warnings),
        _ => {
            return Err(ConfigError::ImportField(format!(
                "windows[{index}] has an unsupported value shape"
            )));
        }
    };

    Ok((window, warnings))
}

fn convert_window_mapping(
    name: String,
    m: &Mapping,
    top_level_pre_window: &[String],
    warnings: &mut Vec<String>,
) -> Window {
    let layout = m.get("layout").and_then(Value::as_str).map(str::to_string);
    let window_root = m.get("root").and_then(Value::as_str).map(str::to_string);

    let pre_window = match m.get("pre_window").or_else(|| m.get("pre")) {
        Some(v) => as_str_list(Some(v)),
        None => top_level_pre_window.to_vec(),
    };

    let panes = match m.get("panes") {
        None => vec![Pane {
            commands: vec![],
            root: window_root.clone(),
        }],
        Some(Value::Sequence(seq)) => seq
            .iter()
            .map(|p| convert_pane(p, window_root.as_deref(), &name, warnings))
            .collect(),
        Some(_) => {
            warnings.push(format!("windows.{name}.panes has an unsupported shape and was imported as a single empty pane"));
            vec![Pane {
                commands: vec![],
                root: window_root.clone(),
            }]
        }
    };

    if window_root.is_some() {
        warnings.push(format!(
            "windows.{name}.root has no facil equivalent at the window level; applied to each of its panes instead"
        ));
    }

    for key in m.keys() {
        if let Some(key_str) = key.as_str()
            && !KNOWN_WINDOW_KEYS.contains(&key_str)
        {
            warnings.push(format!(
                "windows.{name}.{key_str} is not supported by facil and was skipped"
            ));
        }
    }

    Window {
        name,
        layout,
        pre_window,
        panes,
    }
}

fn convert_pane(
    value: &Value,
    window_root: Option<&str>,
    window_name: &str,
    warnings: &mut Vec<String>,
) -> Pane {
    match value {
        Value::String(s) => Pane {
            commands: vec![s.clone()],
            root: window_root.map(str::to_string),
        },
        Value::Null => Pane {
            commands: vec![],
            root: window_root.map(str::to_string),
        },
        Value::Sequence(seq) => Pane {
            commands: seq
                .iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect(),
            root: window_root.map(str::to_string),
        },
        _ => {
            warnings.push(format!(
                "a pane in window `{window_name}` has an unsupported shape and was imported empty"
            ));
            Pane {
                commands: vec![],
                root: window_root.map(str::to_string),
            }
        }
    }
}

fn as_str_list(value: Option<&Value>) -> Vec<String> {
    match value {
        None | Some(Value::Null) => vec![],
        Some(Value::String(s)) => vec![s.clone()],
        Some(Value::Sequence(seq)) => seq
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect(),
        Some(_) => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
name: sample
root: ~/code/sample
pre: docker compose pull
pre_window: rbenv shell 2.0.0-p247

windows:
  - editor:
      layout: main-vertical
      pre_window: source .venv/bin/activate
      panes:
        - vim
        - - cargo watch -x test
          - echo ready
        -
  - server: bundle exec rails s
  - logs:
      root: ~/code/sample/logs
      panes:
        - tail -f dev.log
"#;

    #[test]
    fn converts_representative_config() {
        let (project, warnings) = convert(SAMPLE).unwrap();

        assert_eq!(project.name, "sample");
        assert_eq!(project.root.as_deref(), Some("~/code/sample"));
        assert_eq!(project.pre, vec!["docker compose pull"]);
        assert_eq!(project.windows.len(), 3);

        let editor = &project.windows[0];
        assert_eq!(editor.name, "editor");
        assert_eq!(editor.layout.as_deref(), Some("main-vertical"));
        // window-level pre_window overrides the top-level one
        assert_eq!(editor.pre_window, vec!["source .venv/bin/activate"]);
        assert_eq!(editor.panes.len(), 3);
        assert_eq!(editor.panes[0].commands, vec!["vim"]);
        assert_eq!(
            editor.panes[1].commands,
            vec!["cargo watch -x test", "echo ready"]
        );
        assert_eq!(editor.panes[2].commands, Vec::<String>::new());

        let server = &project.windows[1];
        assert_eq!(server.name, "server");
        // no window-level override -> inherits the top-level pre_window
        assert_eq!(server.pre_window, vec!["rbenv shell 2.0.0-p247"]);
        assert_eq!(server.panes[0].commands, vec!["bundle exec rails s"]);

        let logs = &project.windows[2];
        assert_eq!(logs.panes[0].root.as_deref(), Some("~/code/sample/logs"));

        assert!(warnings.iter().any(|w| w.contains("windows.logs.root")));
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("facil's `pre` runs after"))
        );
    }

    #[test]
    fn unsupported_pane_shape_degrades_with_warning() {
        let yaml = r#"
name: p
windows:
  - w:
      panes:
        - true
"#;
        let (project, warnings) = convert(yaml).unwrap();
        assert_eq!(project.windows[0].panes[0].commands, Vec::<String>::new());
        assert!(warnings.iter().any(|w| w.contains("unsupported shape")));
    }

    #[test]
    fn missing_name_is_hard_error() {
        let yaml = "windows:\n  - w: cmd\n";
        assert!(matches!(convert(yaml), Err(ConfigError::ImportField(_))));
    }

    #[test]
    fn non_sequence_windows_is_hard_error() {
        let yaml = "name: p\nwindows: not-a-list\n";
        assert!(matches!(convert(yaml), Err(ConfigError::ImportField(_))));
    }

    #[test]
    fn unrecognized_top_level_key_warns() {
        let yaml = "name: p\nattach: false\nwindows:\n  - w: cmd\n";
        let (_, warnings) = convert(yaml).unwrap();
        assert!(warnings.iter().any(|w| w.contains("`attach`")));
    }

    #[test]
    fn bare_string_window_shorthand() {
        let yaml = "name: p\nwindows:\n  - solo: echo hi\n";
        let (project, _) = convert(yaml).unwrap();
        assert_eq!(project.windows[0].panes[0].commands, vec!["echo hi"]);
    }
}
