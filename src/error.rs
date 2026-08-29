use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error(transparent)]
    Tmux(#[from] TmuxError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("host command failed: {command} (exit {status})")]
    HostCommand { command: String, status: i32 },
    /// Signals that error details were already printed by the command handler
    /// (e.g. a multi-line validation report); main should just exit nonzero.
    #[error("failed")]
    AlreadyReported,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("config not found: {0}")]
    NotFound(PathBuf),
    #[error("config already exists: {0}")]
    AlreadyExists(PathBuf),
    #[error("failed to parse {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
    #[error("{field}: {message}")]
    Validation { field: String, message: String },
    #[error("unresolved variable `{{{{{0}}}}}` — pass it via --set {0}=value")]
    UnresolvedVariable(String),
    #[error("invalid variable argument `{0}`, expected key=value")]
    InvalidVarArg(String),
    #[error("$HOME is not set")]
    NoHome,
}

#[derive(Debug, thiserror::Error)]
pub enum TmuxError {
    #[error("tmux is not installed or not on PATH")]
    NotFound,
    #[error("failed to run tmux: {0}")]
    Spawn(#[source] std::io::Error),
    #[error("tmux exited with status {status}: {stderr}")]
    CommandFailed { status: i32, stderr: String },
    #[error("tmux did not report a pane id for: {0}")]
    NoPaneId(String),
}
