//! Bridges tool-call permission requests to the UI.
//!
//! The ACP request handler registers each `RequestPermissionRequest` here and
//! awaits the returned receiver; the UI answers from another thread through
//! `resolve` (wired to a Tauri command). Dropping the broker — the session
//! ended — cancels every pending request, which the handler turns into a
//! `Cancelled` outcome.

use std::collections::HashMap;
use std::sync::Mutex;

use agent_client_protocol::schema::v1::{
    PermissionOption, RequestPermissionOutcome, RequestPermissionRequest,
    SelectedPermissionOutcome,
};
use futures::channel::oneshot;

use crate::events::{PermissionOptionInfo, UiEvent, enum_str};

#[derive(Default)]
pub struct PermissionBroker {
    state: Mutex<BrokerState>,
}

#[derive(Default)]
struct BrokerState {
    next_id: u64,
    pending: HashMap<u64, Pending>,
}

struct Pending {
    /// Options offered by the agent, kept to validate the UI's answer.
    options: Vec<PermissionOption>,
    answer: oneshot::Sender<RequestPermissionOutcome>,
}

impl PermissionBroker {
    /// Registers a request and returns the event to show the user plus the
    /// receiver the ACP handler awaits for the decision.
    pub fn begin(
        &self,
        request: &RequestPermissionRequest,
    ) -> (UiEvent, oneshot::Receiver<RequestPermissionOutcome>) {
        let (answer_tx, answer_rx) = oneshot::channel();
        let mut state = self.state.lock().expect("permission broker lock poisoned");
        state.next_id += 1;
        let request_id = state.next_id;
        state.pending.insert(
            request_id,
            Pending {
                options: request.options.clone(),
                answer: answer_tx,
            },
        );

        let event = UiEvent::PermissionRequested {
            request_id,
            tool_title: tool_title(request),
            options: request
                .options
                .iter()
                .map(|option| PermissionOptionInfo {
                    option_id: option.option_id.0.to_string(),
                    name: option.name.clone(),
                    kind: enum_str(&option.kind),
                })
                .collect(),
        };
        (event, answer_rx)
    }

    /// Answers a pending request with the option the user picked. Rejects
    /// answers for unknown requests and options the agent never offered
    /// (stale UI state, duplicated clicks), keeping the request pending in
    /// the latter case so a valid answer can still arrive.
    pub fn resolve(&self, request_id: u64, option_id: &str) -> Result<(), String> {
        let mut state = self.state.lock().expect("permission broker lock poisoned");
        let pending = state.pending.get(&request_id).ok_or_else(|| {
            format!("permission request {request_id} is unknown or already answered")
        })?;
        let selected = pending
            .options
            .iter()
            .find(|option| option.option_id.0.as_ref() == option_id)
            .map(|option| option.option_id.clone())
            .ok_or_else(|| {
                format!("option \"{option_id}\" was not offered for permission request {request_id}")
            })?;
        let pending = state
            .pending
            .remove(&request_id)
            .expect("entry existence checked above");
        pending
            .answer
            .send(RequestPermissionOutcome::Selected(
                SelectedPermissionOutcome::new(selected),
            ))
            .map_err(|_| "the agent session ended before the permission was answered".to_string())
    }

    /// Cancels every pending request. Called when a turn ends: the agent no
    /// longer waits for those answers, so keeping them would only accumulate
    /// dead entries. Dropping the senders resolves the handlers' receivers
    /// with an error, which they answer as `Cancelled`.
    pub fn cancel_pending(&self) {
        self.state
            .lock()
            .expect("permission broker lock poisoned")
            .pending
            .clear();
    }
}

fn tool_title(request: &RequestPermissionRequest) -> String {
    request
        .tool_call
        .fields
        .title
        .clone()
        .unwrap_or_else(|| request.tool_call.tool_call_id.0.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_client_protocol::schema::v1::{
        PermissionOptionKind, ToolCallUpdate, ToolCallUpdateFields,
    };

    fn request_with_options() -> RequestPermissionRequest {
        RequestPermissionRequest::new(
            "s1",
            ToolCallUpdate::new("tc1", ToolCallUpdateFields::default()),
            vec![
                PermissionOption::new("allow", "Allow", PermissionOptionKind::AllowOnce),
                PermissionOption::new("reject", "Reject", PermissionOptionKind::RejectOnce),
            ],
        )
    }

    #[test]
    fn begin_emits_permission_requested_with_mapped_options() {
        let broker = PermissionBroker::default();
        let (event, _answer) = broker.begin(&request_with_options());
        assert_eq!(
            event,
            UiEvent::PermissionRequested {
                request_id: 1,
                tool_title: "tc1".to_string(),
                options: vec![
                    PermissionOptionInfo {
                        option_id: "allow".to_string(),
                        name: "Allow".to_string(),
                        kind: "allow_once".to_string(),
                    },
                    PermissionOptionInfo {
                        option_id: "reject".to_string(),
                        name: "Reject".to_string(),
                        kind: "reject_once".to_string(),
                    },
                ],
            }
        );
    }

    #[test]
    fn begin_prefers_tool_title_over_id() {
        let broker = PermissionBroker::default();
        let mut fields = ToolCallUpdateFields::default();
        fields.title = Some("Run ls".to_string());
        let request = RequestPermissionRequest::new(
            "s1",
            ToolCallUpdate::new("tc1", fields),
            vec![],
        );
        let (event, _answer) = broker.begin(&request);
        let UiEvent::PermissionRequested { tool_title, .. } = event else {
            panic!("expected PermissionRequested");
        };
        assert_eq!(tool_title, "Run ls");
    }

    #[test]
    fn begin_assigns_incrementing_request_ids() {
        let broker = PermissionBroker::default();
        let (first, _a) = broker.begin(&request_with_options());
        let (second, _b) = broker.begin(&request_with_options());
        let ids = [first, second].map(|event| match event {
            UiEvent::PermissionRequested { request_id, .. } => request_id,
            other => panic!("expected PermissionRequested, got {other:?}"),
        });
        assert_eq!(ids, [1, 2]);
    }

    #[test]
    fn resolve_sends_selected_outcome() {
        let broker = PermissionBroker::default();
        let (_event, mut answer) = broker.begin(&request_with_options());

        broker.resolve(1, "allow").expect("resolve should succeed");

        let outcome = answer
            .try_recv()
            .expect("channel should be open")
            .expect("outcome should be sent");
        assert!(matches!(
            outcome,
            RequestPermissionOutcome::Selected(ref selected)
                if selected.option_id.0.as_ref() == "allow"
        ));
    }

    #[test]
    fn resolve_unknown_request_errors() {
        let broker = PermissionBroker::default();
        let error = broker.resolve(42, "allow").unwrap_err();
        assert!(error.contains("42"));
    }

    #[test]
    fn resolve_is_single_shot_per_request() {
        let broker = PermissionBroker::default();
        let (_event, _answer) = broker.begin(&request_with_options());
        broker.resolve(1, "allow").expect("first answer succeeds");
        assert!(broker.resolve(1, "allow").is_err());
    }

    #[test]
    fn resolve_rejects_option_not_offered_and_keeps_request_pending() {
        let broker = PermissionBroker::default();
        let (_event, mut answer) = broker.begin(&request_with_options());

        assert!(broker.resolve(1, "self-approved").is_err());
        assert!(answer.try_recv().expect("still open").is_none());

        broker.resolve(1, "reject").expect("valid answer still lands");
    }

    #[test]
    fn cancel_pending_cancels_answers_and_forgets_requests() {
        let broker = PermissionBroker::default();
        let (_event, mut answer) = broker.begin(&request_with_options());

        broker.cancel_pending();

        assert!(answer.try_recv().is_err());
        assert!(broker.resolve(1, "allow").is_err());
    }

    #[test]
    fn dropping_broker_cancels_pending_answers() {
        let broker = PermissionBroker::default();
        let (_event, mut answer) = broker.begin(&request_with_options());
        drop(broker);
        assert!(answer.try_recv().is_err());
    }
}
