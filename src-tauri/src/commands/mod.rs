//! IPC surface.
//!
//! These are deliberately thin. Each one marshals arguments, calls the engine,
//! and maps errors; no discovery, planning, or ffmpeg logic lives here. If a
//! body grows past roughly thirty lines, the logic belongs in `karui-core`.

pub mod compress;
pub mod devices;
pub mod estimate;
pub mod preview;
pub mod queue;
pub mod sizing;
pub mod tools;
