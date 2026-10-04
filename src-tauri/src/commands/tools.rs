use crate::error::{AppError, Result};
use karui_core::tools::{ToolStatus, Tools};

/// Where ffmpeg is, which version, and which of our codecs it can encode.
///
/// Async because it runs ffmpeg twice, and a sync command would hold the
/// main thread while it did.
#[tauri::command]
pub async fn tool_status() -> Result<ToolStatus> {
    tauri::async_runtime::spawn_blocking(|| Ok(Tools::locate()?.status()?))
        .await
        .map_err(|e| AppError::Invalid(e.to_string()))?
}
