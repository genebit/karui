use crate::error::{AppError, Result};
use crate::state::CardLedger;
use karui_core::devices::{cards, default_import_dir, summarise, Card, CardSummary};
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, State};

/// Sent with the mounted cards whenever one is inserted or removed. Mirrored
/// in `src/lib/ipc.ts`.
pub const DEVICES_EVENT: &str = "devices://changed";

/// How often mounts are checked. A directory listing and a few `stat`s, so
/// polling costs nothing measurable and needs no per-platform notification
/// API.
const POLL: Duration = Duration::from_secs(2);

/// Every mounted camera card, with which of its videos are new.
///
/// Async: reading a card with thousands of clips takes a moment, and a
/// card being mounted can stall a `stat` for longer.
#[tauri::command]
pub async fn list_cards(ledger: State<'_, Arc<CardLedger>>) -> Result<Vec<CardSummary>> {
    let ledger = ledger.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let known = ledger.snapshot();
        cards()
            .into_iter()
            .map(|card| summarise(card, &known))
            .collect()
    })
    .await
    .map_err(|e| AppError::Invalid(e.to_string()))
}

/// Where card videos go when no import folder is chosen.
#[tauri::command]
pub fn default_import() -> Option<String> {
    default_import_dir().map(|dir| dir.display().to_string())
}

/// Watch for cards for the life of the app.
pub fn watch(app: AppHandle) {
    std::thread::spawn(move || {
        let mut last: Vec<Card> = Vec::new();
        loop {
            let now = cards();
            if now != last {
                for card in now.iter().filter(|c| !last.contains(c)) {
                    tracing::info!("Camera card {} connected", card.name);
                }
                let _ = app.emit(DEVICES_EVENT, &now);
                last = now;
            }
            std::thread::sleep(POLL);
        }
    });
}
