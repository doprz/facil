mod cli;
mod commands;
mod config;
mod doctor;
mod error;
mod import;
mod session;
mod snapshot;
mod tmux;

use clap::{CommandFactory, Parser};
use error::Error;

fn main() {
    clap_complete::env::CompleteEnv::with_factory(cli::Cli::command).complete();

    let cli = cli::Cli::parse();
    if let Err(e) = commands::dispatch(cli) {
        if !matches!(e, Error::AlreadyReported) {
            eprintln!("error: {e}");
        }
        std::process::exit(1);
    }
}
