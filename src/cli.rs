use std::path::PathBuf;

use clap::{Parser, Subcommand};

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
        name: Option<String>,
        #[arg(long)]
        no_attach: bool,
        /// key=value for {{var}} substitution, repeatable
        #[arg(short = 's', long = "set", value_name = "KEY=VALUE")]
        vars: Vec<String>,
    },
    /// Kill a running tmux session
    Stop { name: Option<String> },
    /// Scaffold a new config and open it in $EDITOR
    New { name: Option<String> },
    /// Open a config in $EDITOR
    Edit { name: Option<String> },
    /// Show configured projects and whether they're running
    List,
    /// Delete a config file
    Delete { name: Option<String> },
    /// Check a config for errors without touching tmux
    Validate {
        name: Option<String>,
        #[arg(short = 's', long = "set", value_name = "KEY=VALUE")]
        vars: Vec<String>,
    },
    /// Print the tmux command sequence a `start` would run, without executing it
    Debug {
        name: Option<String>,
        #[arg(short = 's', long = "set", value_name = "KEY=VALUE")]
        vars: Vec<String>,
    },
    /// Check that tmux is installed, compatible, and the config dir is writable
    Doctor,
    /// Copy an existing config to a new name and open it in $EDITOR
    Copy { existing: String, new: String },
    /// Print a shell completion script to stdout
    Completions { shell: clap_complete::Shell },
}
