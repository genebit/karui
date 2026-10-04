//! The `karui` engine.
//!
//! Pure Rust with no webview, so it builds and tests on a bare machine. The
//! pipeline runs one direction:
//!
//! ```text
//! paths -> discover -> probe -> plan -> encode (ffmpeg) -> report
//! ```
//!
//! ffmpeg is always spawned with an argument vector, never through a shell.
//! The Python script this replaced built a command string and escaped only
//! spaces and parentheses, so a file name holding a quote or `&` either failed
//! or ran as shell syntax.

pub mod args;
pub mod batch;
pub mod devices;
pub mod discover;
pub mod encode;
pub mod estimate;
pub mod hardware;
pub mod options;
pub mod plan;
pub mod preview;
pub mod probe;
pub mod progress;
pub mod sizing;
pub mod tools;
pub mod units;

mod error;

pub use error::{Error, Result};
