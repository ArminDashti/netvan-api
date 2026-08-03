//! Netvan core: types, database, settings, and IPC protocol.

pub mod db;
pub mod history;
pub mod ipc;
pub mod paths;
pub mod settings;
pub mod types;

pub use db::Database;
pub use history::{HistoryRange, TimeRange};
pub use settings::{AppSettings, AppTheme, CaptureMode, StripSlot};
pub use types::*;
