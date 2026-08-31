use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use clap::CommandFactory;

use crate::cli::{Cli, Commands};
use crate::config;
use crate::config::model::Project;
use crate::config::{substitute, validate};
use crate::doctor;
use crate::error::{ConfigError, Error};
use crate::session;
use crate::tmux::{self, Tmux};

pub fn dispatch(cli: Cli) -> Result<(), Error> {
    let config_override = cli.config.as_deref();
    let verbose = cli.verbose;

    match cli.command {
        Commands::Start {
            name,
            no_attach,
            vars,
        } => start(name, config_override, vars, no_attach, verbose),
        Commands::Stop { name } => stop(name, config_override, verbose),
        Commands::Restart {
            name,
            no_attach,
            vars,
        } => restart(name, config_override, vars, no_attach, verbose),
        Commands::New { name } => new(name, config_override),
        Commands::Edit { name } => edit(name, config_override),
        Commands::List => list(verbose),
        Commands::Delete { name } => delete(name, config_override),
        Commands::Validate { name, vars } => validate_cmd(name, config_override, vars),
        Commands::Debug { name, vars } => debug_cmd(name, config_override, vars),
        Commands::Doctor => doctor::run(),
        Commands::Copy { existing, new } => copy(existing, new),
        Commands::Completions { shell } => completions(shell),
        Commands::Snapshot { session, socket } => snapshot_cmd(session, socket, verbose),
        Commands::Import { path, name } => import_cmd(path, name),
    }
}

/// Run validation, printing every field-level error if any, and signal the
/// caller to exit nonzero without main re-printing a generic message.
fn validate_or_report(project: &Project) -> Result<(), Error> {
    if let Err(errors) = validate::validate(project) {
        for e in &errors {
            eprintln!("{e}");
        }
        return Err(Error::AlreadyReported);
    }
    Ok(())
}

fn load_for_build(
    name: Option<&str>,
    config_override: Option<&std::path::Path>,
    vars: &[String],
) -> Result<Project, Error> {
    let path = config::resolve_path(name, config_override)?;
    let vars = substitute::parse_var_args(vars)?;
    let project = config::load(&path, &vars)?;
    validate_or_report(&project)?;
    Ok(project)
}

fn start(
    name: Option<String>,
    config_override: Option<&std::path::Path>,
    vars: Vec<String>,
    no_attach: bool,
    verbose: u8,
) -> Result<(), Error> {
    let project = load_for_build(name.as_deref(), config_override, &vars)?;
    let tmux = Tmux::new(project.socket_name.clone(), verbose);

    if tmux.has_session(&project.name)? {
        if no_attach {
            println!("session '{}' already running", project.name);
            return Ok(());
        }
        return Ok(tmux.attach_or_switch(&project.name)?);
    }

    build_and_attach(&project, &tmux, no_attach)
}

/// Stop the session first if it's running, then build and attach fresh.
/// Loads and validates the config *before* touching tmux, so a config that
/// broke since the session was started leaves the running session alone
/// rather than killing it and then failing to rebuild it.
fn restart(
    name: Option<String>,
    config_override: Option<&std::path::Path>,
    vars: Vec<String>,
    no_attach: bool,
    verbose: u8,
) -> Result<(), Error> {
    let project = load_for_build(name.as_deref(), config_override, &vars)?;
    let tmux = Tmux::new(project.socket_name.clone(), verbose);

    if tmux.has_session(&project.name)? {
        tmux.kill_session(&project.name)?;
        println!("stopped '{}'", project.name);
    }

    build_and_attach(&project, &tmux, no_attach)
}

fn build_and_attach(project: &Project, tmux: &Tmux, no_attach: bool) -> Result<(), Error> {
    let steps = session::build_plan(project);
    session::execute(&project.name, &steps, tmux)?;

    if no_attach {
        println!("session '{}' started", project.name);
        Ok(())
    } else {
        Ok(tmux.attach_or_switch(&project.name)?)
    }
}

fn stop(
    name: Option<String>,
    config_override: Option<&std::path::Path>,
    verbose: u8,
) -> Result<(), Error> {
    let path = config::resolve_path(name.as_deref(), config_override)?;
    let project = config::load_raw(&path)?;
    let tmux = Tmux::new(project.socket_name.clone(), verbose);

    if tmux.has_session(&project.name)? {
        tmux.kill_session(&project.name)?;
        println!("stopped '{}'", project.name);
    } else {
        println!("no session running for '{}'", project.name);
    }
    Ok(())
}

fn new(name: Option<String>, config_override: Option<&std::path::Path>) -> Result<(), Error> {
    let path = config::target_path(name.as_deref(), config_override)?;
    if path.exists() {
        return Err(ConfigError::AlreadyExists(path).into());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let project_name = name.unwrap_or_else(default_name);
    std::fs::write(&path, config::scaffold(&project_name))?;
    println!("created {}", path.display());

    open_in_editor(&path)
}

fn edit(name: Option<String>, config_override: Option<&std::path::Path>) -> Result<(), Error> {
    let path = config::resolve_path(name.as_deref(), config_override)?;
    open_in_editor(&path)
}

fn open_in_editor(path: &std::path::Path) -> Result<(), Error> {
    match std::env::var("EDITOR") {
        Ok(editor) => {
            std::process::Command::new(editor).arg(path).status()?;
            Ok(())
        }
        Err(_) => {
            println!("$EDITOR is not set; edit {} manually", path.display());
            Ok(())
        }
    }
}

fn list(verbose: u8) -> Result<(), Error> {
    let paths = config::discover_all()?;

    let mut ok_entries: Vec<(String, Option<String>, PathBuf)> = Vec::new();
    let mut error_rows: Vec<(PathBuf, ConfigError)> = Vec::new();
    for path in &paths {
        match config::load_raw(path) {
            Ok(project) => ok_entries.push((project.name, project.socket_name, path.clone())),
            Err(e) => error_rows.push((path.clone(), e)),
        }
    }

    // Query the default socket plus every socket a known config declares. facil
    // has no way to discover sessions on a socket no config has ever mentioned.
    let mut sockets: HashSet<Option<String>> = HashSet::from([None]);
    sockets.extend(ok_entries.iter().map(|(_, socket, _)| socket.clone()));

    let mut live: HashMap<(Option<String>, String), tmux::SessionInfo> = HashMap::new();
    for socket in &sockets {
        for info in Tmux::new(socket.clone(), verbose).list_sessions()? {
            live.insert((socket.clone(), info.name.clone()), info);
        }
    }

    if ok_entries.is_empty() && error_rows.is_empty() && live.is_empty() {
        println!("no configs found and no tmux sessions running");
        return Ok(());
    }

    println!(
        "{:<16} {:<9} {:<9} {:<7} {:<10} {:<8} CONFIG",
        "NAME", "STATUS", "WINDOWS", "PANES", "ATTACHED", "UPTIME"
    );

    let mut matched: HashSet<(Option<String>, String)> = HashSet::new();
    for (name, socket, path) in &ok_entries {
        let key = (socket.clone(), name.clone());
        if let Some(info) = live.get(&key) {
            matched.insert(key);
            print_row(info, &path.display().to_string());
        } else {
            println!(
                "{name:<16} {:<9} {:<9} {:<7} {:<10} {:<8} {}",
                "stopped",
                "-",
                "-",
                "-",
                "-",
                path.display()
            );
        }
    }

    for (path, e) in &error_rows {
        println!(
            "{:<16} {:<9} {:<9} {:<7} {:<10} {:<8} {} ({e})",
            "?",
            "error",
            "-",
            "-",
            "-",
            "-",
            path.display()
        );
    }

    let mut unmanaged: Vec<&tmux::SessionInfo> = live
        .iter()
        .filter(|(key, _)| !matched.contains(key))
        .map(|(_, info)| info)
        .collect();
    unmanaged.sort_by(|a, b| a.name.cmp(&b.name));
    for info in unmanaged {
        print_row(info, "(unmanaged)");
    }

    Ok(())
}

fn print_row(info: &tmux::SessionInfo, config_column: &str) {
    println!(
        "{:<16} {:<9} {:<9} {:<7} {:<10} {:<8} {}",
        info.name,
        "running",
        info.windows,
        info.panes,
        if info.attached { "yes" } else { "no" },
        format_uptime(info.created),
        config_column
    );
}

fn format_uptime(created_epoch: i64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(created_epoch);
    let secs = (now - created_epoch).max(0);

    let days = secs / 86400;
    let hours = (secs % 86400) / 3600;
    let minutes = (secs % 3600) / 60;

    if days > 0 {
        format!("{days}d{hours}h")
    } else if hours > 0 {
        format!("{hours}h{minutes}m")
    } else if minutes > 0 {
        format!("{minutes}m")
    } else {
        format!("{secs}s")
    }
}

fn delete(name: Option<String>, config_override: Option<&std::path::Path>) -> Result<(), Error> {
    let path = config::resolve_path(name.as_deref(), config_override)?;
    std::fs::remove_file(&path)?;
    println!("deleted {}", path.display());
    Ok(())
}

fn validate_cmd(
    name: Option<String>,
    config_override: Option<&std::path::Path>,
    vars: Vec<String>,
) -> Result<(), Error> {
    match load_for_build(name.as_deref(), config_override, &vars) {
        Ok(_) => {
            println!("OK");
            Ok(())
        }
        Err(e) => Err(e),
    }
}

fn debug_cmd(
    name: Option<String>,
    config_override: Option<&std::path::Path>,
    vars: Vec<String>,
) -> Result<(), Error> {
    let project = load_for_build(name.as_deref(), config_override, &vars)?;
    let steps = session::build_plan(&project);
    print!("{}", session::render(&project.name, &steps));
    Ok(())
}

fn copy(existing: String, new: String) -> Result<(), Error> {
    let dir = config::config_dir()?;
    let source = dir.join(format!("{existing}.toml"));
    let dest = dir.join(format!("{new}.toml"));

    if !source.is_file() {
        return Err(ConfigError::NotFound(source).into());
    }
    if dest.exists() {
        return Err(ConfigError::AlreadyExists(dest).into());
    }

    let raw = std::fs::read_to_string(&source)?;
    let rewritten = config::rewrite_name(&raw, &new).ok_or_else(|| ConfigError::Validation {
        field: "name".to_string(),
        message: format!("no top-level `name` field found in {}", source.display()),
    })?;
    std::fs::write(&dest, rewritten)?;
    println!("copied {existing} -> {new}");

    open_in_editor(&dest)
}

fn completions(shell: clap_complete::Shell) -> Result<(), Error> {
    use std::io::Write;

    // Generate into a buffer rather than straight to stdout: clap_complete panics
    // internally on a write failure, which a closed pipe (e.g. `| head`) triggers.
    let mut cmd = Cli::command();
    let mut buf = Vec::new();
    clap_complete::generate(shell, &mut cmd, "facil", &mut buf);

    match std::io::stdout().write_all(&buf) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
        Err(e) => Err(e.into()),
    }
}

fn snapshot_cmd(session: String, socket: Option<String>, verbose: u8) -> Result<(), Error> {
    let dest = config::config_dir()?.join(format!("{session}.toml"));
    if dest.exists() {
        return Err(ConfigError::AlreadyExists(dest).into());
    }

    let tmux = Tmux::new(socket.clone(), verbose);
    let project = crate::snapshot::capture(&tmux, &session, socket)?;
    let body = format!(
        "{}{}",
        crate::snapshot::HEADER_COMMENT,
        toml::to_string_pretty(&project).map_err(ConfigError::from)?
    );

    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&dest, body)?;
    println!("wrote {}", dest.display());

    open_in_editor(&dest)
}

fn import_cmd(path: std::path::PathBuf, name: Option<String>) -> Result<(), Error> {
    let yaml = std::fs::read_to_string(&path).map_err(|_| ConfigError::NotFound(path.clone()))?;
    let (mut project, mut warnings) = crate::import::convert(&yaml)?;

    let dest_name = match name {
        Some(n) => {
            project.name = n.clone();
            n
        }
        None => project.name.clone(),
    };
    let dest = config::config_dir()?.join(format!("{dest_name}.toml"));
    if dest.exists() {
        return Err(ConfigError::AlreadyExists(dest).into());
    }

    if let Err(errors) = validate::validate(&project) {
        warnings.extend(errors.iter().map(|e| format!("validation: {e}")));
    }

    let mut header = String::from("# generated by `facil import` - review before use\n");
    for w in &warnings {
        header.push_str("# - ");
        header.push_str(w);
        header.push('\n');
    }
    header.push('\n');

    let body = format!(
        "{header}{}",
        toml::to_string_pretty(&project).map_err(ConfigError::from)?
    );

    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&dest, body)?;
    println!("imported {} -> {}", path.display(), dest.display());
    for w in &warnings {
        println!("warning: {w}");
    }

    open_in_editor(&dest)
}

fn default_name() -> String {
    std::env::current_dir()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
        .unwrap_or_else(|| "myproject".to_string())
}
