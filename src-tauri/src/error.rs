//! One error type crossing the IPC boundary.
//!
//! The frontend always receives `{ kind, message, detail }`, so it can branch
//! on `kind` without parsing prose. `kind` is a stable machine-readable slug;
//! `message` is what a human reads.

use serde::Serialize;

pub type Result<T> = std::result::Result<T, AppError>;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    Engine(#[from] karui_core::Error),

    #[error("{0}")]
    Io(#[from] std::io::Error),

    #[error("a batch is already running")]
    Busy,

    #[error("{0}")]
    Invalid(String),
}

impl AppError {
    /// A stable slug the frontend can match on.
    fn kind(&self) -> &'static str {
        use karui_core::Error as E;
        match self {
            AppError::Engine(inner) => match inner {
                E::ToolMissing { .. } => "tool-missing",
                E::Tool { .. } => "tool",
                E::Probe { .. } => "probe",
                E::Encode { .. } => "encode",
                E::Invalid(_) => "invalid",
                E::Cancelled => "cancelled",
                E::Io(_) => "io",
            },
            AppError::Io(_) => "io",
            AppError::Busy => "busy",
            AppError::Invalid(_) => "invalid",
        }
    }

    /// Extra context worth showing under the message, when there is any.
    fn detail(&self) -> Option<String> {
        match self {
            AppError::Engine(karui_core::Error::ToolMissing { .. }) => {
                Some(karui_core::tools::install_hint().into())
            }
            _ => None,
        }
    }
}

impl Serialize for AppError {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("AppError", 3)?;
        s.serialize_field("kind", self.kind())?;
        s.serialize_field("message", &self.to_string())?;
        s.serialize_field("detail", &self.detail())?;
        s.end()
    }
}
