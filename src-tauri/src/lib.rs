mod agents;
mod bridge;

use std::path::PathBuf;

use acp_core::{MessageRow, SessionRow, SessionSetup, Store};
use bridge::AcpBridge;
use tauri::{AppHandle, Manager, State};

#[tauri::command]
fn list_agents() -> Vec<agents::AgentListing> {
    agents::listings()
}

/// Returns whether a fresh session was started. `false` means the agent's
/// session was already alive and untouched: the frontend must keep its chat
/// state in that case, because resetting it would drop pending permission
/// cards whose requests the backend still holds open. `force` skips that
/// reuse (the sidebar's "New chat" on the already-running agent).
#[tauri::command]
fn start_session(
    app: AppHandle,
    bridge: State<'_, AcpBridge>,
    agent_name: String,
    force: bool,
) -> Result<bool, String> {
    if !force && bridge.current_agent().as_deref() == Some(agent_name.as_str()) {
        return Ok(false);
    }
    let config = find_agent(&agent_name)?;
    bridge.start(app, config, default_cwd(), SessionSetup::New);
    Ok(true)
}

/// Restores a stored conversation: reconnects to the session's agent in its
/// original working directory and replays history via `session/load`.
#[tauri::command]
fn resume_session(
    app: AppHandle,
    bridge: State<'_, AcpBridge>,
    session_id: String,
) -> Result<(), String> {
    let row = open_store(&bridge)?
        .get_session(&session_id)
        .map_err(|error| format!("{error:#}"))?
        .ok_or_else(|| format!("unknown session \"{session_id}\""))?;
    let config = find_agent(&row.agent_name)?;
    bridge.start(
        app,
        config,
        PathBuf::from(row.cwd),
        SessionSetup::Load { session_id },
    );
    Ok(())
}

#[tauri::command]
fn list_sessions(bridge: State<'_, AcpBridge>) -> Result<Vec<SessionRow>, String> {
    open_store(&bridge)?
        .list_sessions()
        .map_err(|error| format!("{error:#}"))
}

#[tauri::command]
fn load_transcript(
    bridge: State<'_, AcpBridge>,
    session_id: String,
) -> Result<Vec<MessageRow>, String> {
    open_store(&bridge)?
        .load_messages(&session_id)
        .map_err(|error| format!("{error:#}"))
}

#[tauri::command]
fn send_prompt(bridge: State<'_, AcpBridge>, text: String) -> Result<(), String> {
    bridge.send_prompt(text)
}

#[tauri::command]
fn cancel_turn(bridge: State<'_, AcpBridge>) -> Result<(), String> {
    bridge.cancel_turn()
}

#[tauri::command]
fn respond_permission(
    bridge: State<'_, AcpBridge>,
    request_id: u64,
    option_id: String,
) -> Result<(), String> {
    bridge.respond_permission(request_id, &option_id)
}

fn find_agent(agent_name: &str) -> Result<acp_core::AgentConfig, String> {
    agents::builtin_agents()
        .into_iter()
        .find(|agent| agent.name == agent_name)
        .ok_or_else(|| format!("agent \"{agent_name}\" is not available on this machine"))
}

fn open_store(bridge: &AcpBridge) -> Result<Store, String> {
    Store::open(bridge.db_path()).map_err(|error| format!("{error:#}"))
}

fn default_cwd() -> PathBuf {
    std::env::current_dir()
        .or_else(|_| std::env::var("HOME").map(Into::into))
        .unwrap_or_else(|_| "/".into())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let db_path = app.path().app_data_dir()?.join("acp-desk.db");
            if let Some(dir) = db_path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            app.manage(AcpBridge::new(db_path));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_agents,
            start_session,
            resume_session,
            list_sessions,
            load_transcript,
            send_prompt,
            cancel_turn,
            respond_permission
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
