use crate::config;
use crate::error::Error;
use crate::tmux;

/// Every tmux feature facil relies on (`-P -F '#{pane_id}'`, send-keys, split-window,
/// select-layout) predates this by years — it's a conservative floor, not a tight one.
const MIN_TMUX_VERSION: (u32, u32) = (1, 9);

pub fn run() -> Result<(), Error> {
    let mut ok = true;

    match tmux::version() {
        Ok(raw) => {
            println!("tmux found:      ok ({raw})");
            match parse_version(&raw) {
                Some(v) if v >= MIN_TMUX_VERSION => {
                    println!("tmux version:    ok (>= {}.{} required)", MIN_TMUX_VERSION.0, MIN_TMUX_VERSION.1);
                }
                Some((major, minor)) => {
                    ok = false;
                    println!(
                        "tmux version:    fail ({major}.{minor} < {}.{} required)",
                        MIN_TMUX_VERSION.0, MIN_TMUX_VERSION.1
                    );
                }
                None => {
                    ok = false;
                    println!("tmux version:    fail (could not parse version from `{raw}`)");
                }
            }
        }
        Err(e) => {
            ok = false;
            println!("tmux found:      fail ({e})");
            println!("tmux version:    fail (tmux not found)");
        }
    }

    match check_config_dir_writable() {
        Ok(dir) => println!("config dir:      ok ({})", dir.display()),
        Err(e) => {
            ok = false;
            println!("config dir:      fail ({e})");
        }
    }

    if ok { Ok(()) } else { Err(Error::AlreadyReported) }
}

fn check_config_dir_writable() -> Result<std::path::PathBuf, Error> {
    let dir = config::config_dir()?;
    std::fs::create_dir_all(&dir)?;
    let probe = dir.join(".facil-doctor-probe");
    std::fs::write(&probe, b"")?;
    std::fs::remove_file(&probe)?;
    Ok(dir)
}

/// Parse a `tmux -V` string like "tmux 3.7b" or "tmux next-3.4" into (major, minor),
/// ignoring a trailing point-release letter and a leading "next-" branch prefix.
fn parse_version(raw: &str) -> Option<(u32, u32)> {
    let version = raw.strip_prefix("tmux ").unwrap_or(raw).trim();
    let version = version.strip_prefix("next-").unwrap_or(version);

    let (major_str, rest) = version.split_once('.')?;
    let major: u32 = major_str.parse().ok()?;
    let minor_digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    let minor: u32 = minor_digits.parse().ok()?;
    Some((major, minor))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_version() {
        assert_eq!(parse_version("tmux 3.7b"), Some((3, 7)));
    }

    #[test]
    fn parses_next_branch() {
        assert_eq!(parse_version("tmux next-3.4"), Some((3, 4)));
    }

    #[test]
    fn parses_bare_minor() {
        assert_eq!(parse_version("tmux 1.9a"), Some((1, 9)));
    }

    #[test]
    fn rejects_garbage() {
        assert_eq!(parse_version("not a version"), None);
    }

    #[test]
    fn too_old_fails_floor() {
        let v = parse_version("tmux 1.8").unwrap();
        assert!(v < MIN_TMUX_VERSION);
    }
}
