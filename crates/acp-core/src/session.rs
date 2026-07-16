//! Owns one ACP connection: spawn the agent, run the session, pump prompts in
//! and `UiEvent`s out. Dropping the connection kills the child process group,
//! so the caller must treat the running future as owning the agent.

use std::path::PathBuf;
use std::sync::Arc;

use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::schema::v1::{
    CancelNotification, ContentBlock, InitializeRequest, McpServer, McpServerStdio,
    NewSessionRequest, PromptRequest, RequestPermissionOutcome, RequestPermissionRequest,
    RequestPermissionResponse, SessionNotification, TextContent,
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
}

impl AgentConfig {
    fn to_agent(&self) -> AcpAgent {
        AcpAgent::new(McpServer::Stdio(
            McpServerStdio::new(&self.name, &self.command).args(self.args.clone()),
        ))
    }
}

/// Commands the UI feeds into a running session.
#[derive(Debug, Clone, PartialEq)]
pub enum SessionCommand {
    Prompt(String),
    /// Cancels the in-flight turn (`session/cancel`); a no-op while idle.
    Cancel,
}

/// Runs one agent session until the prompt channel closes or the agent dies.
///
/// Not `Send`: run it on a dedicated thread with a current-thread runtime.
/// Events are pushed through `on_event`; prompts arrive on `prompts`.
pub async fn run_session(
    config: AgentConfig,
    cwd: PathBuf,
    mut commands: UnboundedReceiver<SessionCommand>,
    permissions: Arc<PermissionBroker>,
    on_event: impl Fn(UiEvent) + Clone + Send + Sync + 'static,
) -> anyhow::Result<()> {
    let agent = config.to_agent();
    let notification_events = on_event.clone();
    let permission_events = on_event.clone();
    let turn_permissions = Arc::clone(&permissions);

    Client
        .builder()
        .on_receive_notification(
            async move |notification: SessionNotification, _cx| {
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
            let session = cx
                .send_request(NewSessionRequest::new(cwd))
                .block_task()
                .await?;
            on_event(UiEvent::SessionReady {
                session_id: session.session_id.0.to_string(),
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
                        session.session_id.clone(),
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
                                    session.session_id.clone(),
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
