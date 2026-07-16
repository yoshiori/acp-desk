mod agents;
mod bridge;

use bridge::AcpBridge;
use tauri::{AppHandle, State};

#[tauri::command]
fn list_agents() -> Vec<agents::AgentListing> {
    agents::listings()
}

/// Returns whether a fresh session was started. `false` means the agent's
/// session was already alive and untouched: the frontend must keep its chat
/// state in that case, because resetting it would drop pending permission
/// cards whose requests the backend still holds open.
#[tauri::command]
fn start_session(
    app: AppHandle,
    bridge: State<'_, AcpBridge>,
    agent_name: String,
) -> Result<bool, String> {
    if bridge.current_agent().as_deref() == Some(agent_name.as_str()) {
        return Ok(false);
    }
    let config = agents::builtin_agents()
        .into_iter()
        .find(|agent| agent.name == agent_name)
        .ok_or_else(|| format!("agent \"{agent_name}\" is not available on this machine"))?;
    bridge.start(app, config);
    Ok(true)
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(AcpBridge::default())
        .invoke_handler(tauri::generate_handler![
            list_agents,
            start_session,
            send_prompt,
            cancel_turn,
            respond_permission
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
