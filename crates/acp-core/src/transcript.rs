//! Persists the UiEvent stream of one session as a transcript.
//!
//! The recorder mirrors the frontend's chunk merging (same rules as
//! chat-core.ts) so the backend, not the webview, is the source of truth
//! for history: a webview reload can't lose messages. Streamed content is
//! buffered per turn and flushed when the turn ends; store failures are
//! logged and swallowed because persistence must never kill a live session.

use std::time::{SystemTime, UNIX_EPOCH};

use crate::events::{ToolCallDetail, UiEvent};
use crate::store::{MessageRow, Store, text_content_json};

pub struct TranscriptRecorder {
    store: Store,
    agent_name: String,
    cwd: String,
    session_id: Option<String>,
    /// Sessions register lazily on their first persisted row: merely opening
    /// the app (or peeking at a resumed session) must not grow the sidebar
    /// with empty conversations or reorder it.
    registered: bool,
    turn: Vec<Entry>,
}

enum Entry {
    Chunk {
        role: &'static str,
        message_id: Option<String>,
        text: String,
    },
    Tool {
        tool_call_id: String,
        title: String,
        status: String,
        detail: ToolCallDetail,
    },
}

impl TranscriptRecorder {
    pub fn new(store: Store, agent_name: impl Into<String>, cwd: impl Into<String>) -> Self {
        Self {
            store,
            agent_name: agent_name.into(),
            cwd: cwd.into(),
            session_id: None,
            registered: false,
            turn: Vec::new(),
        }
    }

    pub fn observe(&mut self, event: &UiEvent) {
        match event {
            UiEvent::SessionReady { session_id } => {
                self.session_id = Some(session_id.clone());
            }
            UiEvent::UserMessage { text } => {
                // The session loop always ends a turn before accepting the
                // next prompt, but flush defensively so transcript order
                // survives even if that invariant ever changes.
                self.flush_turn();
                let row = MessageRow {
                    role: "user".to_string(),
                    content_json: text_content_json(text),
                    acp_message_id: None,
                    status: None,
                };
                self.with_session(|store, id| store.append_message(id, &row, unix_now()));
            }
            UiEvent::AgentMessageChunk { message_id, text } => {
                self.push_chunk("assistant", message_id.clone(), text);
            }
            UiEvent::AgentThoughtChunk { message_id, text } => {
                self.push_chunk("thought", message_id.clone(), text);
            }
            UiEvent::ToolCall {
                tool_call_id,
                title,
                status,
                detail,
                ..
            } => {
                self.turn.push(Entry::Tool {
                    tool_call_id: tool_call_id.clone(),
                    title: title.clone(),
                    status: status.clone(),
                    detail: detail.clone(),
                });
            }
            UiEvent::ToolCallUpdate {
                tool_call_id,
                title,
                status,
                content_text,
                diffs,
                raw_input_json,
                raw_output_json,
                locations,
            } => {
                let entry = self.turn.iter_mut().rev().find(|entry| {
                    matches!(entry, Entry::Tool { tool_call_id: id, .. } if id == tool_call_id)
                });
                if let Some(Entry::Tool {
                    title: entry_title,
                    status: entry_status,
                    detail: entry_detail,
                    ..
                }) = entry
                {
                    if let Some(title) = title {
                        *entry_title = title.clone();
                    }
                    if let Some(status) = status {
                        *entry_status = status.clone();
                    }
                    // Same replace-if-present contract as the frontend
                    // (chat-core applyEvent), so both transcripts agree.
                    if let Some(text) = content_text {
                        entry_detail.content_text = Some(text.clone());
                    }
                    if let Some(diffs) = diffs {
                        entry_detail.diffs = diffs.clone();
                    }
                    if let Some(raw) = raw_input_json {
                        entry_detail.raw_input_json = Some(raw.clone());
                    }
                    if let Some(raw) = raw_output_json {
                        entry_detail.raw_output_json = Some(raw.clone());
                    }
                    if let Some(locations) = locations {
                        entry_detail.locations = locations.clone();
                    }
                }
            }
            UiEvent::Usage {
                used_tokens,
                context_size,
                cost_amount,
                cost_currency,
            } => {
                let (used, size, amount) = (*used_tokens, *context_size, *cost_amount);
                let currency = cost_currency.clone();
                self.with_session(|store, id| {
                    store.record_usage(id, used, size, amount, currency.as_deref(), unix_now())
                });
            }
            UiEvent::TurnEnded { .. } => self.flush_turn(),
            UiEvent::AgentError { message } => {
                self.flush_turn();
                let row = MessageRow {
                    role: "system".to_string(),
                    content_json: text_content_json(&format!("Agent error: {message}")),
                    acp_message_id: None,
                    status: None,
                };
                self.with_session(|store, id| store.append_message(id, &row, unix_now()));
            }
            // Permission requests are transient UI state; their outcome shows
            // up in the transcript through the tool call's final status.
            UiEvent::PermissionRequested { .. } | UiEvent::AvailableCommands { .. } => {}
        }
    }

    /// Merges a chunk into the buffer following the frontend's rules: same
    /// role and same message id (or both anonymous within the turn) extend
    /// the last entry, anything else starts a new one.
    fn push_chunk(&mut self, role: &'static str, message_id: Option<String>, text: &str) {
        if let Some(Entry::Chunk {
            role: last_role,
            message_id: last_id,
            text: last_text,
        }) = self.turn.last_mut()
            && *last_role == role
            && *last_id == message_id
        {
            last_text.push_str(text);
            return;
        }
        self.turn.push(Entry::Chunk {
            role,
            message_id,
            text: text.to_string(),
        });
    }

    fn flush_turn(&mut self) {
        for entry in std::mem::take(&mut self.turn) {
            let row = match entry {
                Entry::Chunk {
                    role,
                    message_id,
                    text,
                } => MessageRow {
                    role: role.to_string(),
                    content_json: text_content_json(&text),
                    acp_message_id: message_id,
                    status: None,
                },
                Entry::Tool {
                    title,
                    status,
                    detail,
                    ..
                } => MessageRow {
                    role: "tool".to_string(),
                    content_json: tool_content_json(&title, &detail),
                    acp_message_id: None,
                    status: Some(status),
                },
            };
            self.with_session(|store, id| store.append_message(id, &row, unix_now()));
        }
    }

    /// Runs a store operation if the session is known, registering the
    /// session row first (rows reference it) and logging failures. Events
    /// arriving before SessionReady have nowhere to go and are dropped on
    /// purpose.
    fn with_session(&mut self, op: impl FnOnce(&Store, &str) -> anyhow::Result<()>) {
        let Some(session_id) = self.session_id.as_deref() else {
            return;
        };
        if !self.registered {
            match self
                .store
                .record_session(session_id, &self.agent_name, &self.cwd, unix_now())
            {
                Ok(()) => self.registered = true,
                Err(error) => {
                    eprintln!("transcript persistence failed: {error:#}");
                    return;
                }
            }
        }
        if let Err(error) = op(&self.store, session_id) {
            eprintln!("transcript persistence failed: {error:#}");
        }
    }

    #[cfg(test)]
    fn store(&self) -> &Store {
        &self.store
    }
}

/// Tool rows keep the title as a text block and append the detail as its
/// own typed block, so plain-text readers (and rows written before details
/// existed) stay compatible.
fn tool_content_json(title: &str, detail: &ToolCallDetail) -> String {
    if detail.is_empty() {
        return text_content_json(title);
    }
    serde_json::json!([
        { "type": "text", "text": title },
        { "type": "tool_detail", "detail": detail },
    ])
    .to_string()
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recorder() -> TranscriptRecorder {
        TranscriptRecorder::new(Store::open_in_memory().unwrap(), "Claude Code", "/tmp")
    }

    fn ready(recorder: &mut TranscriptRecorder) {
        recorder.observe(&UiEvent::SessionReady {
            session_id: "s1".to_string(),
        });
    }

    fn chunk(text: &str, message_id: Option<&str>) -> UiEvent {
        UiEvent::AgentMessageChunk {
            message_id: message_id.map(str::to_string),
            text: text.to_string(),
        }
    }

    fn texts(recorder: &TranscriptRecorder) -> Vec<(String, String)> {
        recorder
            .store()
            .load_messages("s1")
            .unwrap()
            .into_iter()
            .map(|m| {
                let blocks: serde_json::Value = serde_json::from_str(&m.content_json).unwrap();
                (m.role, blocks[0]["text"].as_str().unwrap().to_string())
            })
            .collect()
    }

    #[test]
    fn full_turn_is_persisted_in_order_with_chunks_merged() {
        let mut recorder = recorder();
        ready(&mut recorder);
        recorder.observe(&UiEvent::UserMessage {
            text: "hi".to_string(),
        });
        recorder.observe(&chunk("Hel", Some("m1")));
        recorder.observe(&chunk("lo", Some("m1")));
        recorder.observe(&UiEvent::TurnEnded {
            stop_reason: "end_turn".to_string(),
        });

        assert_eq!(
            texts(&recorder),
            [
                ("user".to_string(), "hi".to_string()),
                ("assistant".to_string(), "Hello".to_string()),
            ]
        );
    }

    #[test]
    fn message_id_change_starts_a_new_message() {
        let mut recorder = recorder();
        ready(&mut recorder);
        recorder.observe(&chunk("first", Some("m1")));
        recorder.observe(&chunk("second", Some("m2")));
        recorder.observe(&UiEvent::TurnEnded {
            stop_reason: "end_turn".to_string(),
        });
        assert_eq!(texts(&recorder).len(), 2);
    }

    #[test]
    fn thought_and_message_chunks_do_not_merge() {
        let mut recorder = recorder();
        ready(&mut recorder);
        recorder.observe(&UiEvent::AgentThoughtChunk {
            message_id: None,
            text: "hmm".to_string(),
        });
        recorder.observe(&chunk("answer", None));
        recorder.observe(&UiEvent::TurnEnded {
            stop_reason: "end_turn".to_string(),
        });
        let rows = texts(&recorder);
        assert_eq!(rows[0].0, "thought");
        assert_eq!(rows[1].0, "assistant");
    }

    fn tool_call(id: &str, title: &str) -> UiEvent {
        UiEvent::ToolCall {
            tool_call_id: id.to_string(),
            title: title.to_string(),
            kind: "execute".to_string(),
            status: "pending".to_string(),
            detail: ToolCallDetail::default(),
        }
    }

    fn tool_update(id: &str) -> UiEvent {
        UiEvent::ToolCallUpdate {
            tool_call_id: id.to_string(),
            title: None,
            status: None,
            content_text: None,
            diffs: None,
            raw_input_json: None,
            raw_output_json: None,
            locations: None,
        }
    }

    #[test]
    fn tool_call_updates_coalesce_into_one_row_with_final_status() {
        let mut recorder = recorder();
        ready(&mut recorder);
        recorder.observe(&tool_call("tc1", "Run ls"));
        let mut update = tool_update("tc1");
        if let UiEvent::ToolCallUpdate { status, .. } = &mut update {
            *status = Some("completed".to_string());
        }
        recorder.observe(&update);
        recorder.observe(&UiEvent::TurnEnded {
            stop_reason: "end_turn".to_string(),
        });

        let rows = recorder.store().load_messages("s1").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].role, "tool");
        assert_eq!(rows[0].status.as_deref(), Some("completed"));
        // No detail arrived, so the row keeps the plain text-only shape.
        assert!(!rows[0].content_json.contains("tool_detail"));
    }

    #[test]
    fn tool_detail_updates_merge_and_persist_as_a_detail_block() {
        let mut recorder = recorder();
        ready(&mut recorder);
        let mut call = tool_call("tc1", "Write file");
        if let UiEvent::ToolCall { detail, .. } = &mut call {
            detail.raw_input_json = Some("{\"path\": \"a.rs\"}".to_string());
        }
        recorder.observe(&call);
        let mut update = tool_update("tc1");
        if let UiEvent::ToolCallUpdate {
            status,
            content_text,
            diffs,
            ..
        } = &mut update
        {
            *status = Some("completed".to_string());
            *content_text = Some("wrote it".to_string());
            *diffs = Some(vec![crate::events::DiffInfo {
                path: "a.rs".to_string(),
                old_text: None,
                new_text: "fn main() {}".to_string(),
            }]);
        }
        recorder.observe(&update);
        recorder.observe(&UiEvent::TurnEnded {
            stop_reason: "end_turn".to_string(),
        });

        let rows = recorder.store().load_messages("s1").unwrap();
        assert_eq!(rows.len(), 1);
        let blocks: serde_json::Value = serde_json::from_str(&rows[0].content_json).unwrap();
        assert_eq!(blocks[0]["type"], "text");
        assert_eq!(blocks[0]["text"], "Write file");
        assert_eq!(blocks[1]["type"], "tool_detail");
        let detail: ToolCallDetail =
            serde_json::from_value(blocks[1]["detail"].clone()).unwrap();
        assert_eq!(detail.content_text.as_deref(), Some("wrote it"));
        assert_eq!(detail.diffs[0].new_text, "fn main() {}");
        assert_eq!(detail.raw_input_json.as_deref(), Some("{\"path\": \"a.rs\"}"));
    }

    #[test]
    fn agent_error_flushes_the_turn_and_leaves_a_system_row() {
        let mut recorder = recorder();
        ready(&mut recorder);
        recorder.observe(&chunk("partial", None));
        recorder.observe(&UiEvent::AgentError {
            message: "child crashed".to_string(),
        });

        let rows = texts(&recorder);
        assert_eq!(rows[0], ("assistant".to_string(), "partial".to_string()));
        assert_eq!(rows[1].0, "system");
        assert!(rows[1].1.contains("child crashed"));
    }

    #[test]
    fn events_before_session_ready_are_dropped_without_panicking() {
        let mut recorder = recorder();
        recorder.observe(&UiEvent::UserMessage {
            text: "too early".to_string(),
        });
        recorder.observe(&UiEvent::TurnEnded {
            stop_reason: "end_turn".to_string(),
        });
        ready(&mut recorder);
        assert_eq!(texts(&recorder), []);
    }

    #[test]
    fn a_session_with_no_rows_is_not_registered() {
        let mut recorder = recorder();
        ready(&mut recorder);
        recorder.observe(&UiEvent::TurnEnded {
            stop_reason: "end_turn".to_string(),
        });
        assert_eq!(recorder.store().list_sessions().unwrap(), vec![]);
    }

    #[test]
    fn the_first_message_registers_the_session() {
        let mut recorder = recorder();
        ready(&mut recorder);
        recorder.observe(&UiEvent::UserMessage {
            text: "hi".to_string(),
        });
        let sessions = recorder.store().list_sessions().unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].agent_name, "Claude Code");
    }

    #[test]
    fn usage_events_are_recorded_for_the_session() {
        let mut recorder = recorder();
        ready(&mut recorder);
        recorder.observe(&UiEvent::Usage {
            used_tokens: 1500,
            context_size: 200_000,
            cost_amount: Some(0.12),
            cost_currency: Some("USD".to_string()),
        });
        // list_sessions proves the session row exists; the usage row count
        // is asserted through the store's own tests.
        assert_eq!(recorder.store().list_sessions().unwrap().len(), 1);
    }
}
