use std::str::FromStr;

use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::schema::v1::{
    ContentBlock, InitializeRequest, NewSessionRequest, PromptRequest,
    RequestPermissionOutcome, RequestPermissionRequest, RequestPermissionResponse,
    SelectedPermissionOutcome, SessionNotification, TextContent,
};
use agent_client_protocol::{AcpAgent, Agent, Client, ConnectionTo};

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let cmd = std::env::var("ACP_AGENT_CMD")
        .unwrap_or_else(|_| "/home/yoshiori/.npm-global/bin/claude-agent-acp".to_string());
    eprintln!("[spawn] {}", cmd);
    let agent = AcpAgent::from_str(&cmd)?;

    Client
        .builder()
        .on_receive_notification(
            async move |n: SessionNotification, _cx| {
                eprintln!("[update] {:?}", n.update);
                Ok(())
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .on_receive_request(
            async move |req: RequestPermissionRequest, responder, _cx| {
                eprintln!("[permission] {:?}", req);
                let id = req.options.first().map(|o| o.option_id.clone());
                let outcome = id
                    .map(|id| {
                        RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(id))
                    })
                    .unwrap_or(RequestPermissionOutcome::Cancelled);
                responder.respond(RequestPermissionResponse::new(outcome))
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_with(agent, |cx: ConnectionTo<Agent>| async move {
            cx.send_request(InitializeRequest::new(ProtocolVersion::V1))
                .block_task()
                .await?;
            let cwd = std::env::current_dir().map_err(anyhow::Error::from)?;
            let sess = cx
                .send_request(NewSessionRequest::new(cwd))
                .block_task()
                .await?;
            cx.send_request(PromptRequest::new(
                sess.session_id,
                vec![ContentBlock::Text(TextContent::new(
                    "Say hi in one short sentence.".to_string(),
                ))],
            ))
            .block_task()
            .await?;
            Ok(())
        })
        .await?;

    Ok(())
}
