use std::collections::HashSet;

use super::runtime_bridge::parse_finish_reason;
use super::tool_rows::{ToolRow, ToolRowState, tool_target_and_verbs};
use super::util::bounded;
use super::{App, ConversationStatus, MAX_DIAGNOSTIC_BYTES, StreamPart};
use crate::provider::FinishReason;
use crate::runtime::RuntimeEvent;

/// View state isolated per session: switching sessions must not reset the
/// draft, transcript, or scroll position of either side.
#[derive(Debug, Clone)]
pub struct ClientSessionState {
    pub transcript: String,
    pub input_draft: String,
    pub scroll: u16,
    pub loaded_until_seq: i64,
    pub status: ConversationStatus,
    pub active_generation_id: Option<i64>,
    pub text_stream_active: bool,
    pub loading: bool,
    pub current_plan: Vec<(String, String)>,
    pub tool_rows: Vec<ToolRow>,
    pub stream_parts: Vec<StreamPart>,
    pub stream_base_len: Option<usize>,
    pub expanded_tool_rows: HashSet<String>,
}

impl Default for ClientSessionState {
    fn default() -> Self {
        Self {
            transcript: String::new(),
            input_draft: String::new(),
            scroll: 0,
            loaded_until_seq: -1,
            status: ConversationStatus::Idle,
            active_generation_id: None,
            text_stream_active: false,
            loading: false,
            current_plan: Vec::new(),
            tool_rows: Vec::new(),
            stream_parts: Vec::new(),
            stream_base_len: None,
            expanded_tool_rows: HashSet::new(),
        }
    }
}

impl App {
    pub fn active_session_id(&self) -> Option<i64> {
        self.active_session_id
    }

    /// Stash the live view fields into the active session's state.
    pub(crate) fn stash_active(&mut self) {
        self.flush_reasoning();
        self.flush_typewriter();
        let Some(session_id) = self.active_session_id else {
            return;
        };
        let state = self.sessions.entry(session_id).or_default();
        state.transcript = std::mem::take(&mut self.transcript);
        state.input_draft = std::mem::take(&mut self.prompt);
        state.scroll = self.chat_scroll;
        state.loaded_until_seq = self.loaded_until_seq;
        state.status = self.status;
        state.current_plan = self.current_plan.clone();
        state.text_stream_active = self.text_stream_active;
        state.active_generation_id = self.active_generation_id;
        state.tool_rows = std::mem::take(&mut self.tool_rows);
        state.stream_parts = std::mem::take(&mut self.stream_parts);
        state.stream_base_len = self.stream_base_len;
        state.expanded_tool_rows = std::mem::take(&mut self.expanded_tool_rows);
    }

    /// Restore the target session's state into the live view fields.
    pub(crate) fn restore_into_view(&mut self, session_id: i64) {
        let state = self.sessions.entry(session_id).or_default();
        self.transcript = std::mem::take(&mut state.transcript);
        self.prompt = std::mem::take(&mut state.input_draft);
        self.cursor_position = self.prompt.chars().count();
        self.chat_scroll = state.scroll;
        self.loaded_until_seq = state.loaded_until_seq;
        self.status = state.status;
        self.current_plan = state.current_plan.clone();
        self.text_stream_active = state.text_stream_active;
        self.active_generation_id = state.active_generation_id;
        self.stream_parts = std::mem::take(&mut state.stream_parts);
        self.stream_base_len = state.stream_base_len;
        self.expanded_tool_rows = std::mem::take(&mut state.expanded_tool_rows);
        self.tool_rows = std::mem::take(&mut state.tool_rows);
        self.refresh_active_tool();
        self.metrics = None;
        self.diagnostic.clear();
        self.selected_suggestion = 0;

        if self.stream_parts.is_empty()
            && self.transcript.is_empty()
            && let Ok(messages) = self.command_service.session_messages(session_id)
        {
            for msg in messages {
                let trimmed = msg.content.trim();
                if msg.role == "tool" && trimmed.is_empty() {
                    continue;
                }
                if msg.role == "user" {
                    self.stream_parts
                        .push(StreamPart::User(trimmed.to_string()));
                    if !self.transcript.is_empty() && !self.transcript.ends_with("\n\n") {
                        if self.transcript.ends_with('\n') {
                            self.transcript.push('\n');
                        } else {
                            self.transcript.push_str("\n\n");
                        }
                    }
                    self.transcript.push_str(&format!("> {trimmed}\n\n"));
                } else if msg.role == "assistant" {
                    self.stream_parts
                        .push(StreamPart::Text(trimmed.to_string()));
                    if !self.transcript.is_empty() && !self.transcript.ends_with("\n\n") {
                        if self.transcript.ends_with('\n') {
                            self.transcript.push('\n');
                        } else {
                            self.transcript.push_str("\n\n");
                        }
                    }
                    self.transcript.push_str(&format!("{trimmed}\n\n"));
                } else if msg.role == "tool" {
                    let call_id = format!("msg-{}", msg.id);
                    let tool_name = if trimmed.starts_with("Plan updated:") {
                        "update_plan"
                    } else {
                        "tool"
                    };
                    self.upsert_tool_row(
                        &call_id,
                        tool_name,
                        ToolRowState::Completed,
                        trimmed.to_string(),
                        String::new(),
                        None,
                    );
                    if let Some(row) = self.tool_rows.iter_mut().find(|r| r.call_id == call_id) {
                        row.output = trimmed.to_string();
                        row.expandable = row.compute_expandable();
                    }
                    if !self
                        .stream_parts
                        .iter()
                        .any(|p| matches!(p, StreamPart::Tool(id) if id == &call_id))
                    {
                        self.stream_parts.push(StreamPart::Tool(call_id));
                    }
                }
            }
            self.truncate_transcript();
            self.scroll_to_bottom();
        }

        if let Some(runtime) = self.runtime.as_ref() {
            if let Ok(events) = runtime.replay_events_after(session_id, self.loaded_until_seq) {
                for event in events {
                    if event.seq > 0 {
                        self.apply_replayed_event(&event);
                        self.loaded_until_seq = event.seq;
                    }
                }
            }
            if let Some(state) = self.sessions.get_mut(&session_id) {
                state.loaded_until_seq = self.loaded_until_seq;
            }
        }
    }

    pub(crate) fn apply_replayed_event(&mut self, event: &RuntimeEvent) {
        match event.kind.as_str() {
            "assistant_message" => {
                if let Ok(payload) = serde_json::from_str::<serde_json::Value>(&event.payload_json)
                    && let Some(content) = payload.get("content").and_then(|v| v.as_str())
                {
                    let trimmed = content.trim();
                    if !trimmed.is_empty() {
                        let already = self.stream_parts.iter().any(|p| match p {
                            StreamPart::Text(t) => t.contains(trimmed),
                            _ => false,
                        });
                        if !already {
                            self.push_stream_part(StreamPart::Text(trimmed.to_string()));
                        }
                        if !self.transcript.contains(trimmed) {
                            if !self.transcript.is_empty() && !self.transcript.ends_with("\n\n") {
                                if self.transcript.ends_with('\n') {
                                    self.transcript.push('\n');
                                } else {
                                    self.transcript.push_str("\n\n");
                                }
                            }
                            self.transcript.push_str(&format!("{trimmed}\n\n"));
                            self.truncate_transcript();
                            self.scroll_to_bottom();
                        }
                    }
                }
            }
            "user_message" | "prompt_promoted" | "prompt_admitted" => {
                if let Ok(payload) = serde_json::from_str::<serde_json::Value>(&event.payload_json)
                    && let Some(content) = payload
                        .get("content")
                        .or_else(|| payload.get("prompt"))
                        .and_then(|v| v.as_str())
                {
                    let trimmed = content.trim();
                    if !trimmed.is_empty() {
                        let already = self.stream_parts.iter().any(|p| match p {
                            StreamPart::User(u) => u == trimmed,
                            _ => false,
                        });
                        if !already {
                            self.push_stream_part(StreamPart::User(trimmed.to_string()));
                        }
                    }
                }
            }
            "tool_call_created" => {
                if let Ok(payload) = serde_json::from_str::<serde_json::Value>(&event.payload_json)
                {
                    let call_id = payload
                        .get("call_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let tool_name = payload
                        .get("tool_name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("tool");
                    let args = payload.get("arguments");
                    let arguments = args.map(serde_json::Value::to_string).unwrap_or_default();
                    let (_verb, _active_verb, desc) = tool_target_and_verbs(tool_name, args);
                    self.upsert_tool_row(
                        call_id,
                        tool_name,
                        ToolRowState::Pending,
                        desc,
                        arguments,
                        None,
                    );
                    self.push_tool_part(call_id.to_string());
                }
            }
            "tool_call_started" => {
                if let Ok(payload) = serde_json::from_str::<serde_json::Value>(&event.payload_json)
                {
                    let call_id = payload
                        .get("call_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let tool_name = payload
                        .get("tool_name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("tool");
                    let args = payload.get("arguments");
                    let arguments = args.map(serde_json::Value::to_string).unwrap_or_default();
                    let (_verb, _active_verb, desc) = tool_target_and_verbs(tool_name, args);
                    self.upsert_tool_row(
                        call_id,
                        tool_name,
                        ToolRowState::Running,
                        desc,
                        arguments,
                        None,
                    );
                    self.push_tool_part(call_id.to_string());
                }
            }
            "tool_call_settled" => {
                if let Ok(payload) = serde_json::from_str::<serde_json::Value>(&event.payload_json)
                {
                    let call_id = payload
                        .get("call_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let tool_name = payload
                        .get("tool_name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("tool");
                    let status = payload.get("status").and_then(|v| v.as_str()).unwrap_or("");
                    let success = status == "completed";
                    let output = payload
                        .get("result")
                        .and_then(|v| v.as_str())
                        .or_else(|| payload.get("error").and_then(|v| v.as_str()))
                        .unwrap_or("");
                    if !self.complete_tool_row(call_id, tool_name, success, output) {
                        let args = payload.get("arguments");
                        let arguments = args.map(serde_json::Value::to_string).unwrap_or_default();
                        let (_verb, _active_verb, desc) = tool_target_and_verbs(tool_name, args);
                        self.upsert_tool_row(
                            call_id,
                            tool_name,
                            if success {
                                ToolRowState::Completed
                            } else {
                                ToolRowState::Failed
                            },
                            desc,
                            arguments,
                            None,
                        );
                        self.complete_tool_row(call_id, tool_name, success, output);
                    }
                    self.push_tool_part(call_id.to_string());
                    self.refresh_active_tool();
                }
            }
            "tool_executed" => {
                if let Ok(payload) = serde_json::from_str::<serde_json::Value>(&event.payload_json)
                {
                    let call_id = payload.get("id").and_then(|v| v.as_str()).unwrap_or("");
                    let tool_name = payload
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("tool");
                    let success = payload
                        .get("success")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    let output = payload.get("output").and_then(|v| v.as_str()).unwrap_or("");
                    if !self.complete_tool_row(call_id, tool_name, success, output) {
                        let args = payload.get("arguments");
                        let arguments = args.map(serde_json::Value::to_string).unwrap_or_default();
                        let (_verb, _active_verb, desc) = tool_target_and_verbs(tool_name, args);
                        self.upsert_tool_row(
                            call_id,
                            tool_name,
                            if success {
                                ToolRowState::Completed
                            } else {
                                ToolRowState::Failed
                            },
                            desc,
                            arguments,
                            None,
                        );
                        self.complete_tool_row(call_id, tool_name, success, output);
                    }
                    self.push_tool_part(call_id.to_string());
                    self.refresh_active_tool();
                }
            }
            "generation_finished" => {
                for row in &mut self.tool_rows {
                    if matches!(row.state, ToolRowState::Pending | ToolRowState::Running) {
                        row.state = ToolRowState::Failed;
                        row.output = "generation ended before tool completion".to_string();
                    }
                }
                self.active_generation_id = None;
                self.refresh_active_tool();
                if let Ok(payload) = serde_json::from_str::<serde_json::Value>(&event.payload_json)
                    && let Some(status) = payload.get("status").and_then(|v| v.as_str())
                {
                    self.text_stream_active = false;
                    let finish_reason = payload
                        .get("finish_reason")
                        .and_then(|v| v.as_str())
                        .and_then(parse_finish_reason)
                        .or_else(|| self.pending_finish_reason.take());
                    let target_status = match status {
                        "cancelled" => ConversationStatus::Cancelled,
                        "failed" => ConversationStatus::Error,
                        _ => ConversationStatus::Finished(
                            finish_reason.unwrap_or(FinishReason::Stop),
                        ),
                    };
                    self.typewriter.reset();
                    self.status = target_status;
                }
            }
            "error" => {
                if let Ok(payload) = serde_json::from_str::<serde_json::Value>(&event.payload_json)
                    && let Some(message) = payload.get("message").and_then(|v| v.as_str())
                {
                    self.diagnostic = bounded(message.to_string(), MAX_DIAGNOSTIC_BYTES);
                    self.status = ConversationStatus::Error;
                }
            }
            _ => {}
        }
    }

    /// Switch the live view to `session_id`, stashing the current one first.
    /// View state is per session: drafts, transcripts, and status survive
    /// the round trip.
    pub fn switch_session(&mut self, session_id: i64) {
        if self.active_session_id == Some(session_id) {
            return;
        }
        self.stash_active();
        self.active_session_id = Some(session_id);
        self.restore_into_view(session_id);
    }

    /// Per-session state for `session_id` (creating an empty entry on first
    /// access), independent of which session is live.
    pub fn session_state(&mut self, session_id: i64) -> &mut ClientSessionState {
        self.sessions.entry(session_id).or_default()
    }

    pub fn loaded_until_seq(&self) -> i64 {
        self.loaded_until_seq
    }
}
