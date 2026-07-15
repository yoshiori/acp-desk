mod agents;
mod bridge;

use bridge::AcpBridge;
use tauri::{AppHandle, State};

#[tauri::command]
fn list_agents() -> Vec<agents::AgentListing> {
    agents::listings()
}

#[tauri::command]
fn start_session(
    app: AppHandle,
    bridge: State<'_, AcpBridge>,
    agent_name: String,
) -> Result<(), String> {
    if bridge.current_agent().as_deref() == Some(agent_name.as_str()) {
        return Ok(());
    }
    let config = agents::builtin_agents()
        .into_iter()
        .find(|agent| agent.name == agent_name)
        .ok_or_else(|| format!("agent \"{agent_name}\" is not available on this machine"))?;
    bridge.start(app, config);
    Ok(())
}

#[tauri::command]
fn send_prompt(bridge: State<'_, AcpBridge>, text: String) -> Result<(), String> {
    bridge.send_prompt(text)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(AcpBridge::default())
        .invoke_handler(tauri::generate_handler![
            list_agents,
            start_session,
            send_prompt
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
