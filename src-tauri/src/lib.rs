mod agents;
mod bridge;

use std::path::PathBuf;

use acp_core::{
    AgentConfig, AgentRow, AgentSpec, MessageRow, SessionRow, SessionSetup, Store, UsageRow,
    resolve_session_cwd,
};
use bridge::AcpBridge;
use serde::Serialize;
use tauri::{AppHandle, Manager, State};

/// An agent row plus whether its command currently exists on disk (the UI
/// disables unlaunchable agents but still lets the user edit them).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentListing {
    #[serde(flatten)]
    row: AgentRow,
    available: bool,
}

#[tauri::command]
fn list_agents(bridge: State<'_, AcpBridge>) -> Result<Vec<AgentListing>, String> {
    let listings = open_store(&bridge)?
        .list_agents()
        .map_err(|error| format!("{error:#}"))?
        .into_iter()
        .map(|row| AgentListing {
            available: std::path::Path::new(&row.command).is_file(),
            row,
        })
        .collect();
    Ok(listings)
}

#[tauri::command]
fn save_agent(bridge: State<'_, AcpBridge>, spec: AgentSpec) -> Result<i64, String> {
    open_store(&bridge)?
        .save_agent(&spec, unix_now())
        .map_err(|error| format!("{error:#}"))
}

#[tauri::command]
fn delete_agent(bridge: State<'_, AcpBridge>, id: i64) -> Result<(), String> {
    open_store(&bridge)?
        .delete_agent(id)
        .map_err(|error| format!("{error:#}"))
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
    cwd: Option<String>,
) -> Result<bool, String> {
    if !force && bridge.current_agent().as_deref() == Some(agent_name.as_str()) {
        return Ok(false);
    }
    let cwd = resolve_session_cwd(cwd.as_deref(), default_cwd())
        .map_err(|error| format!("{error:#}"))?;
    let config = find_agent(&bridge, &agent_name)?;
    bridge.start(app, config, cwd, SessionSetup::New);
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
    let config = find_agent(&bridge, &row.agent_name)?;
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
fn load_usage(
    bridge: State<'_, AcpBridge>,
    session_id: String,
) -> Result<Option<UsageRow>, String> {
    open_store(&bridge)?
        .last_usage(&session_id)
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

fn find_agent(bridge: &AcpBridge, agent_name: &str) -> Result<AgentConfig, String> {
    let row = open_store(bridge)?
        .get_agent(agent_name)
        .map_err(|error| format!("{error:#}"))?
        .ok_or_else(|| format!("agent \"{agent_name}\" is not configured"))?;
    Ok(AgentConfig {
        name: row.name,
        command: PathBuf::from(row.command),
        args: row.args,
        env: row.env.into_iter().map(|pair| (pair.name, pair.value)).collect(),
    })
}

fn open_store(bridge: &AcpBridge) -> Result<Store, String> {
    Store::open(bridge.db_path()).map_err(|error| format!("{error:#}"))
}

fn default_cwd() -> PathBuf {
    std::env::current_dir()
        .or_else(|_| std::env::var("HOME").map(Into::into))
        .unwrap_or_else(|_| "/".into())
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let db_path = app.path().app_data_dir()?.join("acp-desk.db");
            if let Some(dir) = db_path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            // First launch (or an emptied table) gets the PATH-detected
            // agents; afterwards the table is the single source of truth.
            Store::open(&db_path)?.seed_agents_if_empty(&agents::seed_specs(), unix_now())?;
            app.manage(AcpBridge::new(db_path));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_agents,
            save_agent,
            delete_agent,
            start_session,
            resume_session,
            list_sessions,
            load_transcript,
            load_usage,
            send_prompt,
            cancel_turn,
            respond_permission
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
