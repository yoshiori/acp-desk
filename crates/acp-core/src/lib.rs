//! ACP client logic for acp-desk, kept free of Tauri dependencies so the
//! protocol handling stays unit-testable without webkit system libraries.

mod events;
mod session;

pub use events::{CommandInfo, UiEvent, decide_permission, map_update};
pub use session::{AgentConfig, run_session};
