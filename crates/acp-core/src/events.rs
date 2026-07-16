//! Mapping from ACP session updates to UI-facing events.
//!
//! `UiEvent` is the contract between the Rust backend and the webview: it is
//! what gets serialized into Tauri events, so its serde shape is part of the
//! frontend API and covered by tests.

use agent_client_protocol::schema::v1::{ContentBlock, SessionUpdate, ToolCallContent};
use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum UiEvent {
    AgentMessageChunk {
        message_id: Option<String>,
        text: String,
    },
    AgentThoughtChunk {
        message_id: Option<String>,
        text: String,
    },
    ToolCall {
        tool_call_id: String,
        title: String,
        kind: String,
        status: String,
        detail: ToolCallDetail,
    },
    /// Field semantics follow ACP's update contract: `Some` replaces the
    /// call's previous value, `None` leaves it untouched.
    ToolCallUpdate {
        tool_call_id: String,
        title: Option<String>,
        status: Option<String>,
        content_text: Option<String>,
        diffs: Option<Vec<DiffInfo>>,
        raw_input_json: Option<String>,
        raw_output_json: Option<String>,
        locations: Option<Vec<String>>,
    },
    AvailableCommands {
        commands: Vec<CommandInfo>,
    },
    Usage {
        used_tokens: u64,
        context_size: u64,
        cost_amount: Option<f64>,
        cost_currency: Option<String>,
    },
    /// Asks the user to decide a tool-call permission request. The agent's
    /// turn stays blocked until the UI answers through `respond_permission`
    /// (or the session ends, which cancels the request).
    PermissionRequested {
        request_id: u64,
        tool_title: String,
        options: Vec<PermissionOptionInfo>,
    },
    SessionReady {
        session_id: String,
    },
    /// Echo of a prompt the session actually accepted. The frontend shows
    /// its own copy immediately and ignores this; the transcript recorder
    /// persists this one, so dropped prompts never enter history.
    UserMessage {
        text: String,
    },
    TurnEnded {
        stop_reason: String,
    },
    AgentError {
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CommandInfo {
    pub name: String,
    pub description: String,
}

/// Everything a tool call carries beyond its one-line summary; rendered by
/// the collapsible detail view. Also persisted (as a `tool_detail` content
/// block) so restored sessions keep their details, hence Deserialize.
#[derive(Debug, Clone, Default, Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallDetail {
    pub content_text: Option<String>,
    pub diffs: Vec<DiffInfo>,
    pub raw_input_json: Option<String>,
    pub raw_output_json: Option<String>,
    pub locations: Vec<String>,
}

impl ToolCallDetail {
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }
}

#[derive(Debug, Clone, Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DiffInfo {
    pub path: String,
    pub old_text: Option<String>,
    pub new_text: String,
}

/// One choice of a permission request; `kind` carries the schema's wire name
/// (`allow_once`, `reject_always`, …) so the UI can style allow/reject apart.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PermissionOptionInfo {
    pub option_id: String,
    pub name: String,
    pub kind: String,
}

/// Maps one ACP session update to at most one UI event.
///
/// Updates the v0.1 UI has no representation for (plans, mode changes,
/// user-message echoes, …) map to `None` on purpose; the dispatcher must
/// stay exhaustive because a single session interleaves all variants.
pub fn map_update(update: SessionUpdate) -> Option<UiEvent> {
    match update {
        SessionUpdate::AgentMessageChunk(chunk) => Some(UiEvent::AgentMessageChunk {
            message_id: chunk.message_id.map(|id| id.0.to_string()),
            text: text_of(&chunk.content),
        }),
        SessionUpdate::AgentThoughtChunk(chunk) => Some(UiEvent::AgentThoughtChunk {
            message_id: chunk.message_id.map(|id| id.0.to_string()),
            text: text_of(&chunk.content),
        }),
        SessionUpdate::ToolCall(call) => Some(UiEvent::ToolCall {
            tool_call_id: call.tool_call_id.0.to_string(),
            title: call.title,
            kind: enum_str(&call.kind),
            status: enum_str(&call.status),
            detail: ToolCallDetail {
                content_text: content_text_of(&call.content),
                diffs: diffs_of(&call.content),
                raw_input_json: call.raw_input.as_ref().map(pretty_json),
                raw_output_json: call.raw_output.as_ref().map(pretty_json),
                locations: call.locations.iter().map(location_str).collect(),
            },
        }),
        SessionUpdate::ToolCallUpdate(update) => Some(UiEvent::ToolCallUpdate {
            tool_call_id: update.tool_call_id.0.to_string(),
            title: update.fields.title,
            status: update.fields.status.as_ref().map(enum_str),
            // A replaced content list with no text blocks still replaces the
            // text (with ""), so map to Some unconditionally when present.
            content_text: update
                .fields
                .content
                .as_deref()
                .map(|content| content_text_of(content).unwrap_or_default()),
            diffs: update.fields.content.as_deref().map(diffs_of),
            raw_input_json: update.fields.raw_input.as_ref().map(pretty_json),
            raw_output_json: update.fields.raw_output.as_ref().map(pretty_json),
            locations: update
                .fields
                .locations
                .map(|locations| locations.iter().map(location_str).collect()),
        }),
        SessionUpdate::AvailableCommandsUpdate(update) => Some(UiEvent::AvailableCommands {
            commands: update
                .available_commands
                .into_iter()
                .map(|c| CommandInfo {
                    name: c.name,
                    description: c.description,
                })
                .collect(),
        }),
        SessionUpdate::UsageUpdate(usage) => Some(UiEvent::Usage {
            used_tokens: usage.used,
            context_size: usage.size,
            cost_amount: usage.cost.as_ref().map(|c| c.amount),
            cost_currency: usage.cost.map(|c| c.currency),
        }),
        // No v0.1 UI for these; SessionUpdate is #[non_exhaustive] so the
        // wildcard also absorbs future variants instead of breaking the build.
        _ => None,
    }
}

fn text_of(content: &ContentBlock) -> String {
    match content {
        ContentBlock::Text(text) => text.text.clone(),
        other => format!("[unsupported content: {}]", enum_str(other)),
    }
}

/// Joined text of the plain-content blocks; None when there are none
/// (diff/terminal blocks are surfaced separately).
fn content_text_of(content: &[ToolCallContent]) -> Option<String> {
    let texts: Vec<String> = content
        .iter()
        .filter_map(|block| match block {
            ToolCallContent::Content(inner) => Some(text_of(&inner.content)),
            _ => None,
        })
        .collect();
    (!texts.is_empty()).then(|| texts.join("\n"))
}

fn diffs_of(content: &[ToolCallContent]) -> Vec<DiffInfo> {
    content
        .iter()
        .filter_map(|block| match block {
            ToolCallContent::Diff(diff) => Some(DiffInfo {
                path: diff.path.display().to_string(),
                old_text: diff.old_text.clone(),
                new_text: diff.new_text.clone(),
            }),
            _ => None,
        })
        .collect()
}

fn location_str(location: &agent_client_protocol::schema::v1::ToolCallLocation) -> String {
    match location.line {
        Some(line) => format!("{}:{line}", location.path.display()),
        None => location.path.display().to_string(),
    }
}

fn pretty_json(value: &serde_json::Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}

/// Snake_case wire name of a serde enum variant (e.g. `ToolCallStatus::InProgress`
/// → `"in_progress"`), reusing the schema crate's own serialization.
pub(crate) fn enum_str<T: Serialize>(value: &T) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(s)) => s,
        // Content blocks and friends serialize to objects with a "type" tag.
        Ok(serde_json::Value::Object(map)) => map
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string(),
        _ => "unknown".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_client_protocol::schema::v1::{
        AvailableCommand, AvailableCommandsUpdate, ContentChunk, Cost, Diff, MessageId,
        TextContent, ToolCall, ToolCallLocation, ToolCallStatus, ToolCallUpdate,
        ToolCallUpdateFields, UsageUpdate,
    };

    fn text_chunk(text: &str, message_id: Option<&str>) -> ContentChunk {
        let mut chunk = ContentChunk::new(ContentBlock::Text(TextContent::new(text.to_string())));
        chunk.message_id = message_id.map(|id| MessageId::from(id.to_string()));
        chunk
    }

    #[test]
    fn agent_message_chunk_maps_text_and_message_id() {
        let update = SessionUpdate::AgentMessageChunk(text_chunk("hello", Some("m1")));
        assert_eq!(
            map_update(update),
            Some(UiEvent::AgentMessageChunk {
                message_id: Some("m1".to_string()),
                text: "hello".to_string(),
            })
        );
    }

    #[test]
    fn agent_message_chunk_without_message_id_maps_to_none_id() {
        let update = SessionUpdate::AgentMessageChunk(text_chunk("hi", None));
        let Some(UiEvent::AgentMessageChunk { message_id, .. }) = map_update(update) else {
            panic!("expected AgentMessageChunk");
        };
        assert_eq!(message_id, None);
    }

    #[test]
    fn thought_chunk_maps_to_thought_event() {
        let update = SessionUpdate::AgentThoughtChunk(text_chunk("thinking", Some("t1")));
        assert!(matches!(
            map_update(update),
            Some(UiEvent::AgentThoughtChunk { .. })
        ));
    }

    #[test]
    fn usage_update_mid_turn_has_no_cost() {
        let update = SessionUpdate::UsageUpdate(UsageUpdate::new(1200, 200_000));
        assert_eq!(
            map_update(update),
            Some(UiEvent::Usage {
                used_tokens: 1200,
                context_size: 200_000,
                cost_amount: None,
                cost_currency: None,
            })
        );
    }

    #[test]
    fn usage_update_final_carries_cost() {
        let update =
            SessionUpdate::UsageUpdate(UsageUpdate::new(1500, 200_000).cost(Cost::new(0.118, "USD")));
        let Some(UiEvent::Usage {
            cost_amount,
            cost_currency,
            ..
        }) = map_update(update)
        else {
            panic!("expected Usage");
        };
        assert_eq!(cost_amount, Some(0.118));
        assert_eq!(cost_currency, Some("USD".to_string()));
    }

    #[test]
    fn tool_call_maps_ids_and_wire_names() {
        let update = SessionUpdate::ToolCall(ToolCall::new("tc1", "Run ls"));
        assert_eq!(
            map_update(update),
            Some(UiEvent::ToolCall {
                tool_call_id: "tc1".to_string(),
                title: "Run ls".to_string(),
                kind: "other".to_string(),
                status: "pending".to_string(),
                detail: ToolCallDetail::default(),
            })
        );
    }

    #[test]
    fn tool_call_maps_detail_fields() {
        let mut call = ToolCall::new("tc1", "Write file");
        call.content = vec![
            ToolCallContent::from(ContentBlock::Text(TextContent::new("wrote it"))),
            ToolCallContent::Diff(Diff::new("/tmp/a.rs", "new body")),
        ];
        call.locations = vec![ToolCallLocation::new("/tmp/a.rs")];
        call.raw_input = Some(serde_json::json!({"path": "/tmp/a.rs"}));

        let Some(UiEvent::ToolCall { detail, .. }) = map_update(SessionUpdate::ToolCall(call))
        else {
            panic!("expected ToolCall");
        };
        assert_eq!(detail.content_text.as_deref(), Some("wrote it"));
        assert_eq!(detail.diffs.len(), 1);
        assert_eq!(detail.diffs[0].path, "/tmp/a.rs");
        assert_eq!(detail.diffs[0].old_text, None);
        assert_eq!(detail.diffs[0].new_text, "new body");
        assert_eq!(detail.locations, vec!["/tmp/a.rs".to_string()]);
        assert!(detail.raw_input_json.as_deref().unwrap().contains("\"path\""));
        assert_eq!(detail.raw_output_json, None);
    }

    #[test]
    fn tool_call_location_with_line_is_rendered_into_the_path() {
        let mut call = ToolCall::new("tc1", "Read");
        let mut location = ToolCallLocation::new("/tmp/a.rs");
        location.line = Some(42);
        call.locations = vec![location];
        let Some(UiEvent::ToolCall { detail, .. }) = map_update(SessionUpdate::ToolCall(call))
        else {
            panic!("expected ToolCall");
        };
        assert_eq!(detail.locations, vec!["/tmp/a.rs:42".to_string()]);
    }

    #[test]
    fn tool_call_update_maps_changed_fields_only() {
        let mut fields = ToolCallUpdateFields::default();
        fields.status = Some(ToolCallStatus::InProgress);
        let update = ToolCallUpdate::new("tc1", fields);
        let Some(UiEvent::ToolCallUpdate {
            title,
            status,
            content_text,
            diffs,
            raw_input_json,
            raw_output_json,
            locations,
            ..
        }) = map_update(SessionUpdate::ToolCallUpdate(update))
        else {
            panic!("expected ToolCallUpdate");
        };
        assert_eq!(title, None);
        assert_eq!(status, Some("in_progress".to_string()));
        assert_eq!(content_text, None);
        assert_eq!(diffs, None);
        assert_eq!(raw_input_json, None);
        assert_eq!(raw_output_json, None);
        assert_eq!(locations, None);
    }

    #[test]
    fn tool_call_update_carries_present_detail_fields() {
        let mut fields = ToolCallUpdateFields::default();
        fields.status = Some(ToolCallStatus::Completed);
        fields.raw_output = Some(serde_json::json!({"ok": true}));
        fields.content = Some(vec![ToolCallContent::from(ContentBlock::Text(
            TextContent::new("done"),
        ))]);
        let update = ToolCallUpdate::new("tc1", fields);
        let Some(UiEvent::ToolCallUpdate {
            content_text,
            diffs,
            raw_output_json,
            ..
        }) = map_update(SessionUpdate::ToolCallUpdate(update))
        else {
            panic!("expected ToolCallUpdate");
        };
        assert_eq!(content_text, Some("done".to_string()));
        assert_eq!(diffs, Some(vec![]));
        assert!(raw_output_json.unwrap().contains("\"ok\""));
    }

    #[test]
    fn available_commands_map_to_command_info() {
        let update = SessionUpdate::AvailableCommandsUpdate(AvailableCommandsUpdate::new(vec![
            AvailableCommand::new("compact", "Compact the conversation"),
        ]));
        assert_eq!(
            map_update(update),
            Some(UiEvent::AvailableCommands {
                commands: vec![CommandInfo {
                    name: "compact".to_string(),
                    description: "Compact the conversation".to_string(),
                }],
            })
        );
    }

    #[test]
    fn permission_requested_serializes_with_camel_case_options() {
        let event = UiEvent::PermissionRequested {
            request_id: 7,
            tool_title: "Run ls".to_string(),
            options: vec![PermissionOptionInfo {
                option_id: "allow".to_string(),
                name: "Allow".to_string(),
                kind: "allow_once".to_string(),
            }],
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["type"], "permission_requested");
        assert_eq!(json["requestId"], 7);
        assert_eq!(json["toolTitle"], "Run ls");
        assert_eq!(json["options"][0]["optionId"], "allow");
        assert_eq!(json["options"][0]["kind"], "allow_once");
    }

    #[test]
    fn ui_event_serializes_with_camel_case_fields_and_type_tag() {
        let event = UiEvent::AgentMessageChunk {
            message_id: Some("m1".to_string()),
            text: "hi".to_string(),
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["type"], "agent_message_chunk");
        assert_eq!(json["messageId"], "m1");
        assert_eq!(json["text"], "hi");
    }
}
