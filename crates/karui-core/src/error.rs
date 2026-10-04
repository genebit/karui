use std::path::PathBuf;
use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    /// Not on `PATH`, not named by its environment variable, and not in any
    /// of the usual install locations.
    #[error("{tool} not found")]
    ToolMissing { tool: &'static str },

    /// The tool was found but did not behave as expected.
    #[error("{tool}: {message}")]
    Tool { tool: &'static str, message: String },

    #[error("could not read {}: {message}", path.display())]
    Probe { path: PathBuf, message: String },

    #[error("ffmpeg failed on {}: {message}", path.display())]
    Encode { path: PathBuf, message: String },

    #[error("{0}")]
    Invalid(String),

    #[error("cancelled")]
    Cancelled,

    #[error(transparent)]
    Io(#[from] std::io::Error),
}
