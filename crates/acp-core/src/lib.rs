//! ACP client logic for acp-desk, kept free of Tauri dependencies so the
//! protocol handling stays unit-testable without webkit system libraries.

mod events;
mod permission;
mod session;

pub use events::{CommandInfo, PermissionOptionInfo, UiEvent, map_update};
pub use permission::PermissionBroker;
pub use session::{AgentConfig, run_session};
