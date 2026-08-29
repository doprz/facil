use std::collections::HashMap;

use crate::error::ConfigError;

/// Replace `{{var}}` occurrences in `raw` using `vars`. Unrecognized identifiers
/// are left untouched so the caller can decide whether leftovers are an error.
pub fn substitute(raw: &str, vars: &HashMap<String, String>) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;

    while let Some(start) = rest.find("{{") {
        let Some(end_rel) = rest[start + 2..].find("}}") else {
            out.push_str(rest);
            return out;
        };
        let end = start + 2 + end_rel;
        let name = rest[start + 2..end].trim();

        out.push_str(&rest[..start]);
        match vars.get(name) {
            Some(value) => out.push_str(value),
            None => out.push_str(&rest[start..end + 2]),
        }
        rest = &rest[end + 2..];
    }
    out.push_str(rest);
    out
}

/// Find any remaining `{{name}}` tokens, returning the first as an error.
pub fn check_no_unresolved(raw: &str) -> Result<(), ConfigError> {
    if let Some(start) = raw.find("{{")
        && let Some(end_rel) = raw[start + 2..].find("}}")
    {
        let end = start + 2 + end_rel;
        let name = raw[start + 2..end].trim();
        return Err(ConfigError::UnresolvedVariable(name.to_string()));
    }
    Ok(())
}

/// Parse `key=value` CLI arguments into a substitution map.
pub fn parse_var_args(args: &[String]) -> Result<HashMap<String, String>, ConfigError> {
    let mut vars = HashMap::new();
    for arg in args {
        let (key, value) = arg
            .split_once('=')
            .ok_or_else(|| ConfigError::InvalidVarArg(arg.clone()))?;
        if key.is_empty() {
            return Err(ConfigError::InvalidVarArg(arg.clone()));
        }
        vars.insert(key.to_string(), value.to_string());
    }
    Ok(vars)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn substitutes_known_vars() {
        let mut vars = HashMap::new();
        vars.insert("branch".to_string(), "main".to_string());
        let out = substitute("checkout {{branch}} now", &vars);
        assert_eq!(out, "checkout main now");
    }

    #[test]
    fn leaves_unknown_vars_untouched() {
        let vars = HashMap::new();
        let out = substitute("checkout {{branch}} now", &vars);
        assert_eq!(out, "checkout {{branch}} now");
    }

    #[test]
    fn detects_unresolved() {
        let err = check_no_unresolved("checkout {{branch}} now").unwrap_err();
        assert!(matches!(err, ConfigError::UnresolvedVariable(name) if name == "branch"));
    }

    #[test]
    fn parses_var_args() {
        let vars = parse_var_args(&["branch=main".to_string(), "port=8080".to_string()]).unwrap();
        assert_eq!(vars.get("branch").unwrap(), "main");
        assert_eq!(vars.get("port").unwrap(), "8080");
    }

    #[test]
    fn rejects_bad_var_arg() {
        assert!(parse_var_args(&["nokey".to_string()]).is_err());
    }
}
