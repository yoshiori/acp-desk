//! Owns one ACP connection: spawn the agent, run the session, pump prompts in
//! and `UiEvent`s out. Dropping the connection kills the child process group,
//! so the caller must treat the running future as owning the agent.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::schema::v1::{
    CancelNotification, ContentBlock, EnvVariable, InitializeRequest, LoadSessionRequest,
    McpServer, McpServerStdio, NewSessionRequest, PromptRequest, RequestPermissionOutcome,
    RequestPermissionRequest, RequestPermissionResponse, SessionId, SessionNotification,
    TextContent,
};
use agent_client_protocol::{AcpAgent, Agent, Client, ConnectionTo};
use futures::channel::mpsc::UnboundedReceiver;
use futures::{FutureExt, StreamExt, select};
use serde::{Deserialize, Serialize};

use crate::events::{UiEvent, map_update};
use crate::permission::PermissionBroker;

/// A launchable agent. `command` must be an absolute path: the app may not
/// inherit the shell's PATH (Tauri launched from a desktop entry, cargo run,
/// …), so PATH lookups fail in exactly the environments users hit first.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AgentConfig {
    pub name: String,
    pub command: PathBuf,
    pub args: Vec<String>,
    /// Extra environment for the child (e.g. GEMINI_API_KEY); appended to
    /// the inherited environment.
    #[serde(default)]
    pub env: Vec<(String, String)>,
}

impl AgentConfig {
    fn to_agent(&self) -> AcpAgent {
        let env: Vec<EnvVariable> = self
            .env
            .iter()
            .map(|(name, value)| EnvVariable::new(name.clone(), value.clone()))
            .collect();
        AcpAgent::new(McpServer::Stdio(
            McpServerStdio::new(&self.name, &self.command)
                .args(self.args.clone())
                .env(env),
        ))
    }
}

/// Resolves the working directory for a new session: an explicit request
/// must be an existing absolute directory (a typo'd cwd would otherwise
/// surface as a confusing agent-spawn failure); no request means fallback.
pub fn resolve_session_cwd(
    requested: Option<&str>,
    fallback: PathBuf,
) -> anyhow::Result<PathBuf> {
    let Some(requested) = requested else {
        return Ok(fallback);
    };
    let path = PathBuf::from(requested);
    anyhow::ensure!(
        path.is_absolute(),
        "working directory must be an absolute path"
    );
    anyhow::ensure!(
        path.is_dir(),
        "working directory does not exist: {requested}"
    );
    Ok(path)
}

/// Commands the UI feeds into a running session.
#[derive(Debug, Clone, PartialEq)]
pub enum SessionCommand {
    Prompt(String),
    /// Cancels the in-flight turn (`session/cancel`); a no-op while idle.
    Cancel,
}

/// How to obtain the ACP session: create a fresh one, or restore a stored
/// conversation with `session/load` (requires the agent's `loadSession`
/// capability, e.g. claude-agent-acp).
#[derive(Debug, Clone, PartialEq)]
pub enum SessionSetup {
    New,
    Load { session_id: String },
}

/// Runs one agent session until the prompt channel closes or the agent dies.
///
/// Not `Send`: run it on a dedicated thread with a current-thread runtime.
/// Events are pushed through `on_event`; prompts arrive on `prompts`.
pub async fn run_session(
    config: AgentConfig,
    cwd: PathBuf,
    setup: SessionSetup,
    mut commands: UnboundedReceiver<SessionCommand>,
    permissions: Arc<PermissionBroker>,
    on_event: impl Fn(UiEvent) + Clone + Send + Sync + 'static,
) -> anyhow::Result<()> {
    let agent = config.to_agent();
    let notification_events = on_event.clone();
    let permission_events = on_event.clone();
    let turn_permissions = Arc::clone(&permissions);
    // session/load replays the whole conversation as notifications before it
    // responds. The transcript store already holds that history (and the UI
    // hydrates from it), so replayed events are dropped: forwarding them
    // would duplicate every message in both the UI and the database.
    let replaying = Arc::new(AtomicBool::new(false));
    let replay_gate = Arc::clone(&replaying);

    Client
        .builder()
        .on_receive_notification(
            async move |notification: SessionNotification, _cx| {
                if replay_gate.load(Ordering::Relaxed) {
                    return Ok(());
                }
                if let Some(event) = map_update(notification.update) {
                    notification_events(event);
                }
                Ok(())
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .on_receive_request(
            async move |request: RequestPermissionRequest, responder, cx| {
                let (event, answer) = permissions.begin(&request);
                permission_events(event);
                // The user may take arbitrarily long to answer, and handlers
                // block the connection's event loop; await the decision in a
                // spawned task so notifications keep streaming meanwhile.
                cx.spawn(async move {
                    let outcome = answer
                        .await
                        .unwrap_or(RequestPermissionOutcome::Cancelled);
                    responder.respond(RequestPermissionResponse::new(outcome))
                })
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_with(agent, |cx: ConnectionTo<Agent>| async move {
            cx.send_request(InitializeRequest::new(ProtocolVersion::V1))
                .block_task()
                .await?;
            let session_id: SessionId = match setup {
                SessionSetup::New => {
                    cx.send_request(NewSessionRequest::new(cwd))
                        .block_task()
                        .await?
                        .session_id
                }
                SessionSetup::Load { session_id } => {
                    replaying.store(true, Ordering::Relaxed);
                    let loaded = cx
                        .send_request(LoadSessionRequest::new(session_id.clone(), cwd))
                        .block_task()
                        .await;
                    replaying.store(false, Ordering::Relaxed);
                    loaded?;
                    SessionId::new(session_id)
                }
            };
            on_event(UiEvent::SessionReady {
                session_id: session_id.0.to_string(),
            });

            // False once the command channel closes (the session was
            // replaced): the in-flight turn is cancelled and awaited so the
            // child exits through the normal turn-end path instead of
            // leaking, then the loop stops.
            let mut commands_open = true;

            while commands_open {
                let text = match commands.next().await {
                    Some(SessionCommand::Prompt(text)) => text,
                    // Nothing is in flight while idle.
                    Some(SessionCommand::Cancel) => continue,
                    None => break,
                };
                on_event(UiEvent::UserMessage { text: text.clone() });

                let turn = cx
                    .send_request(PromptRequest::new(
                        session_id.clone(),
                        vec![ContentBlock::Text(TextContent::new(text))],
                    ))
                    .block_task()
                    .fuse();
                futures::pin_mut!(turn);

                // Keep listening for Cancel while the turn runs. The agent
                // answers a cancelled turn with stop_reason "cancelled", so
                // cancellation still flows out through the response below.
                // One session/cancel per turn is enough: repeated Stop
                // clicks or a channel close right after a manual cancel
                // must not spam the agent.
                let mut cancel_sent = false;
                let response = loop {
                    if !commands_open {
                        break (&mut turn).await;
                    }
                    select! {
                        response = &mut turn => break response,
                        command = commands.next() => {
                            match command {
                                // The UI refuses to send prompts while busy;
                                // drop any that race through.
                                Some(SessionCommand::Prompt(_)) => continue,
                                Some(SessionCommand::Cancel) => {}
                                None => commands_open = false,
                            }
                            if !cancel_sent {
                                cancel_sent = true;
                                // Unblock pending permission dialogs first
                                // or the agent may never get to process the
                                // cancellation.
                                turn_permissions.cancel_pending();
                                cx.send_notification(CancelNotification::new(
                                    session_id.clone(),
                                ))?;
                            }
                        },
                    }
                };
                let response = response?;

                // The turn is over, so nobody waits on unanswered permission
                // requests anymore; drop them instead of accumulating.
                turn_permissions.cancel_pending();
                on_event(UiEvent::TurnEnded {
                    stop_reason: stop_reason_str(&response.stop_reason),
                });
            }
            Ok(())
        })
        .await?;

    Ok(())
}

fn stop_reason_str<T: Serialize>(reason: &T) -> String {
    serde_json::to_value(reason)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_else(|| "unknown".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_request_uses_the_fallback() {
        let cwd = resolve_session_cwd(None, PathBuf::from("/fallback")).unwrap();
        assert_eq!(cwd, PathBuf::from("/fallback"));
    }

    #[test]
    fn an_existing_absolute_directory_is_accepted() {
        let dir = std::env::temp_dir();
        let cwd = resolve_session_cwd(Some(dir.to_str().unwrap()), "/fallback".into()).unwrap();
        assert_eq!(cwd, dir);
    }

    #[test]
    fn relative_and_missing_directories_are_rejected() {
        assert!(resolve_session_cwd(Some("relative/path"), "/f".into()).is_err());
        assert!(resolve_session_cwd(Some("/no/such/dir/exists/here"), "/f".into()).is_err());
    }
}
