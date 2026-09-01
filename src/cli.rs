use std::path::PathBuf;

use clap::{Parser, Subcommand};
use clap_complete::engine::{ArgValueCompleter, CompletionCandidate};

use crate::config;

/// Existing config names under `~/.config/facil/*.toml` starting with `current`.
/// Used to dynamically complete arguments that select an *existing* config -
/// never attached where the name being typed doesn't exist yet (`new`'s name,
/// `copy`'s `new`) or already has its own well-defined fallback (`import`'s name).
fn config_name_completer(current: &std::ffi::OsStr) -> Vec<CompletionCandidate> {
    let Some(current) = current.to_str() else {
        return vec![];
    };
    let Ok(dir) = config::config_dir() else {
        return vec![];
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return vec![];
    };

    entries
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let path = e.path();
            if path.extension()?.to_str()? != "toml" {
                return None;
            }
            path.file_stem()?.to_str().map(str::to_string)
        })
        .filter(|name| name.starts_with(current))
        .map(CompletionCandidate::new)
        .collect()
}

#[derive(Parser)]
#[command(name = "facil", version, about = "TOML-driven tmux session templating")]
pub struct Cli {
    /// Increase verbosity (-v shows tmux commands, -vv also shows their output)
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    pub verbose: u8,

    /// Explicit config file path, overrides name-based discovery
    #[arg(long, global = true, value_name = "PATH")]
    pub config: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Build (or attach to) a tmux session from a config
    Start {
        #[arg(add = ArgValueCompleter::new(config_name_completer))]
        name: Option<String>,
        #[arg(long)]
        no_attach: bool,
        /// key=value for {{var}} substitution, repeatable
        #[arg(short = 's', long = "set", value_name = "KEY=VALUE")]
        vars: Vec<String>,
    },
    /// Kill a running tmux session
    Stop {
        #[arg(add = ArgValueCompleter::new(config_name_completer))]
        name: Option<String>,
    },
    /// Stop the session if running, then start it fresh from the config
    Restart {
        #[arg(add = ArgValueCompleter::new(config_name_completer))]
        name: Option<String>,
        #[arg(long)]
        no_attach: bool,
        /// key=value for {{var}} substitution, repeatable
        #[arg(short = 's', long = "set", value_name = "KEY=VALUE")]
        vars: Vec<String>,
    },
    /// Scaffold a new config and open it in $EDITOR
    New { name: Option<String> },
    /// Open a config in $EDITOR
    Edit {
        #[arg(add = ArgValueCompleter::new(config_name_completer))]
        name: Option<String>,
    },
    /// Show configured projects and live tmux sessions, matched up where possible
    #[command(alias = "ls")]
    List,
    /// Delete a config file
    Delete {
        #[arg(add = ArgValueCompleter::new(config_name_completer))]
        name: Option<String>,
    },
    /// Check a config for errors without touching tmux
    Validate {
        #[arg(add = ArgValueCompleter::new(config_name_completer))]
        name: Option<String>,
        #[arg(short = 's', long = "set", value_name = "KEY=VALUE")]
        vars: Vec<String>,
    },
    /// Print the tmux command sequence a `start` would run, without executing it
    Debug {
        #[arg(add = ArgValueCompleter::new(config_name_completer))]
        name: Option<String>,
        #[arg(short = 's', long = "set", value_name = "KEY=VALUE")]
        vars: Vec<String>,
    },
    /// Check that tmux is installed, compatible, and the config dir is writable
    Doctor,
    /// Copy an existing config to a new name and open it in $EDITOR
    Copy {
        #[arg(add = ArgValueCompleter::new(config_name_completer))]
        existing: String,
        new: String,
    },
    /// Print a shell completion script to stdout
    Completions { shell: clap_complete::Shell },
    /// Write a facil config that reproduces a live tmux session's window/pane
    /// layout and working directories (not commands, pre/post, or tmux_options)
    Snapshot {
        session: String,
        /// tmux socket the session lives on (matches tmux's -L)
        #[arg(long)]
        socket: Option<String>,
    },
    /// Convert a tmuxinator YAML config to a facil config and open it in $EDITOR
    Import {
        path: PathBuf,
        /// output config name; defaults to the YAML's own top-level `name`
        name: Option<String>,
    },
}
