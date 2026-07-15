//! Owns one ACP connection: spawn the agent, run the session, pump prompts in
//! and `UiEvent`s out. Dropping the connection kills the child process group,
//! so the caller must treat the running future as owning the agent.

use std::path::PathBuf;

use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::schema::v1::{
    ContentBlock, InitializeRequest, McpServer, McpServerStdio, NewSessionRequest, PromptRequest,
    RequestPermissionRequest, RequestPermissionResponse, SessionNotification, TextContent,
};
use agent_client_protocol::{AcpAgent, Agent, Client, ConnectionTo};
use futures::StreamExt;
use futures::channel::mpsc::UnboundedReceiver;
use serde::{Deserialize, Serialize};

use crate::events::{UiEvent, decide_permission, map_update};

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

/// Runs one agent session until the prompt channel closes or the agent dies.
///
/// Not `Send`: run it on a dedicated thread with a current-thread runtime.
/// Events are pushed through `on_event`; prompts arrive on `prompts`.
pub async fn run_session(
    config: AgentConfig,
    cwd: PathBuf,
    mut prompts: UnboundedReceiver<String>,
    on_event: impl Fn(UiEvent) + Clone + Send + Sync + 'static,
) -> anyhow::Result<()> {
    let agent = config.to_agent();
    let notification_events = on_event.clone();
    let permission_events = on_event.clone();

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
            async move |request: RequestPermissionRequest, responder, _cx| {
                let (outcome, event) = decide_permission(&request);
                permission_events(event);
                responder.respond(RequestPermissionResponse::new(outcome))
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

            while let Some(text) = prompts.next().await {
                let response = cx
                    .send_request(PromptRequest::new(
                        session.session_id.clone(),
                        vec![ContentBlock::Text(TextContent::new(text))],
                    ))
                    .block_task()
                    .await?;
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
