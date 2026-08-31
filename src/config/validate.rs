use std::collections::HashSet;

use crate::config::model::Project;
use crate::error::ConfigError;

pub fn validate(project: &Project) -> Result<(), Vec<ConfigError>> {
    let mut errors = Vec::new();

    if project.name.trim().is_empty() {
        errors.push(field_error("name", "must not be empty"));
    }

    if project.windows.is_empty() {
        errors.push(field_error("windows", "at least one window is required"));
    }

    if let Some(root) = &project.root {
        check_dir_exists("root", root, &mut errors);
    }

    let mut seen_names = HashSet::new();
    for (wi, window) in project.windows.iter().enumerate() {
        let prefix = format!("windows[{wi}]");

        if window.name.trim().is_empty() {
            errors.push(field_error(&format!("{prefix}.name"), "must not be empty"));
        } else if !seen_names.insert(window.name.clone()) {
            errors.push(field_error(
                &format!("{prefix}.name"),
                &format!("duplicate window name `{}`", window.name),
            ));
        }

        if let Some(layout) = &window.layout
            && layout.trim().is_empty()
        {
            errors.push(field_error(
                &format!("{prefix}.layout"),
                "must not be empty if set",
            ));
        }

        for (pi, pane) in window.panes.iter().enumerate() {
            if let Some(root) = &pane.root {
                check_dir_exists(&format!("{prefix}.panes[{pi}].root"), root, &mut errors);
            }
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn check_dir_exists(field: &str, raw: &str, errors: &mut Vec<ConfigError>) {
    let path = crate::config::model::expand_tilde(raw);
    if !path.is_dir() {
        errors.push(field_error(
            field,
            &format!("directory does not exist: {}", path.display()),
        ));
    }
}

fn field_error(field: &str, message: &str) -> ConfigError {
    ConfigError::Validation {
        field: field.to_string(),
        message: message.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::model::{Pane, Window};

    fn base_project() -> Project {
        Project {
            name: "myproject".to_string(),
            root: None,
            pre: vec![],
            post: vec![],
            tmux_options: None,
            socket_name: None,
            windows: vec![Window {
                name: "editor".to_string(),
                layout: None,
                pre_window: vec![],
                panes: vec![Pane {
                    commands: vec!["nvim .".to_string()],
                    root: None,
                }],
            }],
        }
    }

    #[test]
    fn valid_project_passes() {
        assert!(validate(&base_project()).is_ok());
    }

    #[test]
    fn empty_windows_fails() {
        let mut project = base_project();
        project.windows.clear();
        let errors = validate(&project).unwrap_err();
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, ConfigError::Validation { field, .. } if field == "windows"))
        );
    }

    #[test]
    fn duplicate_window_names_fail() {
        let mut project = base_project();
        let dup = Window {
            name: "editor".to_string(),
            layout: None,
            pre_window: vec![],
            panes: vec![],
        };
        project.windows.push(dup);
        let errors = validate(&project).unwrap_err();
        assert!(
            errors.iter().any(
                |e| matches!(e, ConfigError::Validation { field, .. } if field.contains("name"))
            )
        );
    }

    #[test]
    fn missing_root_dir_fails() {
        let mut project = base_project();
        project.root = Some("/definitely/does/not/exist/facil-test".to_string());
        let errors = validate(&project).unwrap_err();
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, ConfigError::Validation { field, .. } if field == "root"))
        );
    }
}
