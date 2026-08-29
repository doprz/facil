use crate::cli::{Cli, Commands};
use crate::config;
use crate::config::model::Project;
use crate::config::{substitute, validate};
use crate::error::{ConfigError, Error};
use crate::session;
use crate::tmux::Tmux;

pub fn dispatch(cli: Cli) -> Result<(), Error> {
    let config_override = cli.config.as_deref();
    let verbose = cli.verbose;

    match cli.command {
        Commands::Start { name, no_attach, vars } => start(name, config_override, vars, no_attach, verbose),
        Commands::Stop { name } => stop(name, config_override, verbose),
        Commands::New { name } => new(name, config_override),
        Commands::Edit { name } => edit(name, config_override),
        Commands::List => list(verbose),
        Commands::Delete { name } => delete(name, config_override),
        Commands::Validate { name, vars } => validate_cmd(name, config_override, vars),
        Commands::Debug { name, vars } => debug_cmd(name, config_override, vars),
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

fn load_for_build(name: Option<&str>, config_override: Option<&std::path::Path>, vars: &[String]) -> Result<Project, Error> {
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

    let steps = session::build_plan(&project);
    session::execute(&project.name, &steps, &tmux)?;

    if no_attach {
        println!("session '{}' started", project.name);
        Ok(())
    } else {
        Ok(tmux.attach_or_switch(&project.name)?)
    }
}

fn stop(name: Option<String>, config_override: Option<&std::path::Path>, verbose: u8) -> Result<(), Error> {
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
    if paths.is_empty() {
        println!("no configs found");
        return Ok(());
    }

    println!("{:<20} {:<10} PATH", "NAME", "STATUS");
    for path in paths {
        match config::load_raw(&path) {
            Ok(project) => {
                let tmux = Tmux::new(project.socket_name.clone(), verbose);
                let running = tmux.has_session(&project.name)?;
                let status = if running { "running" } else { "stopped" };
                println!("{:<20} {:<10} {}", project.name, status, path.display());
            }
            Err(e) => {
                println!("{:<20} {:<10} {} ({e})", "?", "error", path.display());
            }
        }
    }
    Ok(())
}

fn delete(name: Option<String>, config_override: Option<&std::path::Path>) -> Result<(), Error> {
    let path = config::resolve_path(name.as_deref(), config_override)?;
    std::fs::remove_file(&path)?;
    println!("deleted {}", path.display());
    Ok(())
}

fn validate_cmd(name: Option<String>, config_override: Option<&std::path::Path>, vars: Vec<String>) -> Result<(), Error> {
    match load_for_build(name.as_deref(), config_override, &vars) {
        Ok(_) => {
            println!("OK");
            Ok(())
        }
        Err(e) => Err(e),
    }
}

fn debug_cmd(name: Option<String>, config_override: Option<&std::path::Path>, vars: Vec<String>) -> Result<(), Error> {
    let project = load_for_build(name.as_deref(), config_override, &vars)?;
    let steps = session::build_plan(&project);
    print!("{}", session::render(&project.name, &steps));
    Ok(())
}

fn default_name() -> String {
    std::env::current_dir()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
        .unwrap_or_else(|| "myproject".to_string())
}
