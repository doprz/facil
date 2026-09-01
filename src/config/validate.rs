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

    if let Err(message) = project.resolve_attach_window() {
        errors.push(field_error("attach_window", &message));
    }

    let mut seen_names = HashSet::new();
    for (wi, window) in project.windows.iter().enumerate() {
        let prefix = format!("windows[{wi}]");

        match &window.name {
            Some(name) if name.trim().is_empty() => {
                errors.push(field_error(
                    &format!("{prefix}.name"),
                    "must not be empty if set",
                ));
            }
            Some(name) if !seen_names.insert(name.clone()) => {
                errors.push(field_error(
                    &format!("{prefix}.name"),
                    &format!("duplicate window name `{name}`"),
                ));
            }
            _ => {}
        }

        if let Some(layout) = &window.layout
            && layout.trim().is_empty()
        {
            errors.push(field_error(
                &format!("{prefix}.layout"),
                "must not be empty if set",
            ));
        }

        if let Some(root) = &window.root {
            check_dir_exists(&format!("{prefix}.root"), root, &mut errors);
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
            attach_window: None,
            windows: vec![Window {
                name: Some("editor".to_string()),
                layout: None,
                root: None,
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
    fn unnamed_windows_never_collide() {
        let mut project = base_project();
        project.windows[0].name = None;
        project.windows.push(Window {
            name: None,
            layout: None,
            root: None,
            pre_window: vec![],
            panes: vec![],
        });
        assert!(validate(&project).is_ok());
    }

    #[test]
    fn duplicate_window_names_fail() {
        let mut project = base_project();
        let dup = Window {
            name: Some("editor".to_string()),
            layout: None,
            root: None,
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

    #[test]
    fn missing_window_root_dir_fails() {
        let mut project = base_project();
        project.windows[0].root = Some("/definitely/does/not/exist/facil-test".to_string());
        let errors = validate(&project).unwrap_err();
        assert!(errors.iter().any(
            |e| matches!(e, ConfigError::Validation { field, .. } if field == "windows[0].root")
        ));
    }

    #[test]
    fn attach_window_by_name_passes() {
        let mut project = base_project();
        project.attach_window = Some("editor".to_string());
        assert!(validate(&project).is_ok());
    }

    #[test]
    fn attach_window_by_position_passes() {
        let mut project = base_project();
        project.attach_window = Some("1".to_string());
        assert!(validate(&project).is_ok());
    }

    #[test]
    fn attach_window_invalid_fails() {
        let mut project = base_project();
        project.attach_window = Some("nope".to_string());
        let errors = validate(&project).unwrap_err();
        assert!(errors.iter().any(
            |e| matches!(e, ConfigError::Validation { field, .. } if field == "attach_window")
        ));
    }
}
