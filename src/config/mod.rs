pub mod model;
pub mod substitute;
pub mod validate;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use model::Project;

use crate::error::{ConfigError, Error};

const LOCAL_CONFIG: &str = "facil.toml";

pub fn config_dir() -> Result<PathBuf, ConfigError> {
    let home = std::env::var("HOME").map_err(|_| ConfigError::NoHome)?;
    Ok(PathBuf::from(home).join(".config").join("facil"))
}

/// Resolve which config file a `name` (or lack thereof) and `--config` override refer to.
/// Does not require the file to exist.
pub fn target_path(name: Option<&str>, override_path: Option<&Path>) -> Result<PathBuf, ConfigError> {
    if let Some(p) = override_path {
        return Ok(p.to_path_buf());
    }
    match name {
        Some(n) => Ok(config_dir()?.join(format!("{n}.toml"))),
        None => Ok(PathBuf::from(LOCAL_CONFIG)),
    }
}

/// Resolve and require the config file to already exist.
pub fn resolve_path(name: Option<&str>, override_path: Option<&Path>) -> Result<PathBuf, ConfigError> {
    let path = target_path(name, override_path)?;
    if path.is_file() {
        Ok(path)
    } else {
        Err(ConfigError::NotFound(path))
    }
}

fn read_and_substitute(path: &Path, vars: &HashMap<String, String>) -> Result<String, ConfigError> {
    let raw = std::fs::read_to_string(path).map_err(|_| ConfigError::NotFound(path.to_path_buf()))?;
    Ok(substitute::substitute(&raw, vars))
}

/// Parse a config without requiring variable substitution to be complete.
/// Suitable for commands that only need top-level fields (e.g. `stop`).
pub fn load_raw(path: &Path) -> Result<Project, ConfigError> {
    let resolved = read_and_substitute(path, &HashMap::new())?;
    toml::from_str(&resolved).map_err(|source| ConfigError::Parse { path: path.to_path_buf(), source })
}

/// Parse a config, substituting `vars` and requiring no `{{var}}` tokens remain.
/// Suitable for commands that build a session (`start`, `debug`, `validate`).
pub fn load(path: &Path, vars: &HashMap<String, String>) -> Result<Project, ConfigError> {
    let resolved = read_and_substitute(path, vars)?;
    substitute::check_no_unresolved(&resolved)?;
    toml::from_str(&resolved).map_err(|source| ConfigError::Parse { path: path.to_path_buf(), source })
}

pub fn scaffold(name: &str) -> String {
    format!(
        r#"name = "{name}"
root = "~/code/{name}"

[[windows]]
name = "editor"
layout = "main-vertical"

[[windows.panes]]
commands = ["nvim ."]

[[windows.panes]]
commands = ["cargo watch -x test"]
"#
    )
}

/// Rewrite the top-level `name = "..."` line to `new_name`, leaving everything else
/// (including any window's own `name` field) untouched. TOML's grammar guarantees
/// every root-table key appears before the file's first `[`-prefixed table header,
/// so tracking that boundary is exact, not a heuristic. Returns `None` if no
/// top-level `name` assignment was found.
pub fn rewrite_name(raw: &str, new_name: &str) -> Option<String> {
    let mut out = String::with_capacity(raw.len() + new_name.len());
    let mut in_root_table = true;
    let mut replaced = false;

    for line in raw.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if trimmed.starts_with('[') {
            in_root_table = false;
        }

        if in_root_table
            && !replaced
            && let Some(rest) = trimmed.strip_prefix("name")
            && rest.trim_start().starts_with('=')
        {
            let newline = if line.ends_with('\n') { "\n" } else { "" };
            out.push_str(&format!("name = \"{new_name}\"{newline}"));
            replaced = true;
            continue;
        }

        out.push_str(line);
    }

    replaced.then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPEC_EXAMPLE: &str = r#"
name = "myproject"
root = "~/code/myproject"

[[windows]]
name = "editor"
layout = "main-vertical"

[[windows.panes]]
commands = ["nvim ."]

[[windows.panes]]
commands = ["cargo watch -x test"]

[[windows]]
name = "server"

[[windows.panes]]
commands = ["docker compose up"]
"#;

    #[test]
    fn parses_spec_example() {
        let project: Project = toml::from_str(SPEC_EXAMPLE).unwrap();
        assert_eq!(project.name, "myproject");
        assert_eq!(project.root.as_deref(), Some("~/code/myproject"));
        assert_eq!(project.windows.len(), 2);

        let editor = &project.windows[0];
        assert_eq!(editor.name, "editor");
        assert_eq!(editor.layout.as_deref(), Some("main-vertical"));
        assert_eq!(editor.panes.len(), 2);
        assert_eq!(editor.panes[0].commands, vec!["nvim ."]);
        assert_eq!(editor.panes[1].commands, vec!["cargo watch -x test"]);

        let server = &project.windows[1];
        assert_eq!(server.name, "server");
        assert!(server.layout.is_none());
        assert_eq!(server.panes.len(), 1);
        assert_eq!(server.panes[0].commands, vec!["docker compose up"]);
    }

    #[test]
    fn rewrite_name_updates_only_top_level_name() {
        let out = rewrite_name(SPEC_EXAMPLE, "renamed").unwrap();
        let project: Project = toml::from_str(&out).unwrap();
        assert_eq!(project.name, "renamed");
        assert_eq!(project.windows[0].name, "editor");
        assert_eq!(project.windows[1].name, "server");
    }

    #[test]
    fn rewrite_name_none_when_missing() {
        let raw = "root = \"/tmp\"\n\n[[windows]]\nname = \"a\"\n";
        assert_eq!(rewrite_name(raw, "renamed"), None);
    }

    #[test]
    fn substitutes_before_parsing() {
        let raw = "name = \"{{proj}}\"\nwindows = []\n";
        let mut vars = HashMap::new();
        vars.insert("proj".to_string(), "resolved".to_string());
        let substituted = substitute::substitute(raw, &vars);
        let project: Project = toml::from_str(&substituted).unwrap();
        assert_eq!(project.name, "resolved");
    }
}

pub fn discover_all() -> Result<Vec<PathBuf>, Error> {
    let mut paths = Vec::new();

    if Path::new(LOCAL_CONFIG).is_file() {
        paths.push(PathBuf::from(LOCAL_CONFIG));
    }

    let dir = config_dir()?;
    if dir.is_dir() {
        let mut entries: Vec<PathBuf> = std::fs::read_dir(&dir)?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|ext| ext == "toml"))
            .collect();
        entries.sort();
        paths.extend(entries);
    }

    Ok(paths)
}
