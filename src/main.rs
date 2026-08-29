mod cli;
mod commands;
mod config;
mod doctor;
mod error;
mod session;
mod tmux;

use clap::Parser;
use error::Error;

fn main() {
    let cli = cli::Cli::parse();
    if let Err(e) = commands::dispatch(cli) {
        if !matches!(e, Error::AlreadyReported) {
            eprintln!("error: {e}");
        }
        std::process::exit(1);
    }
}
