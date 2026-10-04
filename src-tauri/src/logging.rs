//! Forward `tracing` events to the UI's log panel.
//!
//! A custom `Layer` turns each event into a `log://line` Tauri event, so the
//! engine's `tracing::warn!` calls surface in the app without the engine
//! knowing anything about Tauri.

use serde::Serialize;
use std::collections::VecDeque;
use std::fmt::Write as _;
use std::sync::{Arc, Mutex, PoisonError};
use tauri::{AppHandle, Emitter};
use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::{Context, Layer};

pub const LOG_EVENT: &str = "log://line";

/// How many lines to keep for a client that has not connected yet.
const BACKLOG_LIMIT: usize = 2000;

/// Mirrored by `LogLine` in `src/lib/bindings.ts`.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogLine {
    pub level: String,
    pub message: String,
}

/// Lines emitted before the window could subscribe.
///
/// `setup` logs before the webview mounts its listener; without a backlog
/// those lines would be lost.
#[derive(Clone, Default)]
pub struct Backlog(Arc<Mutex<VecDeque<LogLine>>>);

impl Backlog {
    pub fn push(&self, line: LogLine) {
        let mut lines = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if lines.len() >= BACKLOG_LIMIT {
            lines.pop_front();
        }
        lines.push_back(line);
    }

    pub fn take(&self) -> Vec<LogLine> {
        std::mem::take(&mut *self.0.lock().unwrap_or_else(PoisonError::into_inner)).into()
    }
}

pub struct UiLayer {
    handle: AppHandle,
    backlog: Backlog,
}

impl UiLayer {
    pub fn new(handle: AppHandle, backlog: Backlog) -> Self {
        Self { handle, backlog }
    }
}

/// Collects an event's `message` field, ignoring structured fields the panel
/// has no use for.
#[derive(Default)]
struct MessageVisitor(String);

impl Visit for MessageVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            let _ = write!(self.0, "{value:?}");
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.0.push_str(value);
        }
    }
}

impl<S: Subscriber> Layer<S> for UiLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let mut visitor = MessageVisitor::default();
        event.record(&mut visitor);
        if visitor.0.is_empty() {
            return;
        }

        let line = LogLine {
            level: event.metadata().level().to_string().to_lowercase(),
            message: visitor.0,
        };

        self.backlog.push(line.clone());
        // A failure to emit means the window has gone; nothing useful to do.
        let _ = self.handle.emit(LOG_EVENT, line);
    }
}

/// Hand the frontend everything logged before it started listening.
#[tauri::command]
pub fn log_backlog(backlog: tauri::State<'_, Backlog>) -> Vec<LogLine> {
    backlog.take()
}
