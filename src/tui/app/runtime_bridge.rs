use super::tool_rows::{ActiveToolInfo, ToolRowState, tool_names_match, tool_target_and_verbs};
use super::util::{bounded, parse_plan_items};
use super::{App, ConversationStatus, MAX_DIAGNOSTIC_BYTES, MAX_TOOL_ARGUMENT_BYTES, StreamPart};
use crate::provider::FinishReason;
use crate::runtime::client::RuntimeClient;
use crate::runtime::{EventBus, RuntimeEvent};

pub(crate) fn parse_finish_reason(value: &str) -> Option<FinishReason> {
    if value.eq_ignore_ascii_case("stop") {
        Some(FinishReason::Stop)
    } else if value.eq_ignore_ascii_case("length") || value.eq_ignore_ascii_case("max_tokens") {
        Some(FinishReason::Length)
    } else if value.eq_ignore_ascii_case("tool_call")
        || value.eq_ignore_ascii_case("tool_calls")
        || value.eq_ignore_ascii_case("tool_use")
    {
        Some(FinishReason::ToolCall)
    } else if value.eq_ignore_ascii_case("error") {
        Some(FinishReason::Error)
    } else {
        None
    }
}

impl App {
    pub(crate) fn request_runtime_cancel(&self) {
        if let Some(runtime) = self.runtime.as_ref()
            && let Some(session_id) = self.active_session_id
        {
            let _ = runtime.cancel_generation(session_id);
        }
    }

    /// Consumes a UI cancellation request. Provider cancellation is not wired yet.
    pub fn take_cancellation(&mut self) -> bool {
        std::mem::take(&mut self.cancellation_pending)
    }

    /// Attach a background runtime client to this app. Prompts for the active
    /// session are then queued as generations and progress arrives as
    /// runtime events (drained by [`App::poll_runtime`]).
    pub fn attach_runtime(
        &mut self,
        db: crate::persistence::Db,
        writer: crate::persistence::WriterHandle,
        provider: Box<dyn crate::provider::Provider + Send + Sync>,
    ) {
        let bus = EventBus::new();
        let (_subscription_id, receiver) = bus.subscribe(None);
        let client = RuntimeClient::spawn(db, writer, provider, bus);
        self.runtime_events = Some(receiver);
        self.runtime = Some(client);
    }

    pub fn set_runtime_receiver(&mut self, receiver: std::sync::mpsc::Receiver<RuntimeEvent>) {
        self.runtime_events = Some(receiver);
    }

    pub fn set_command_service(
        &mut self,
        service: crate::cli::CommandService<crate::cli::CliDiscovery>,
    ) {
        self.command_service = service;
    }

    fn handle_runtime_event(&mut self, event: RuntimeEvent) {
        if let Some(session_id) = self.active_session_id
            && event.session_id != session_id
        {
            return;
        }
        if event.seq > 0 {
            if event.seq <= self.loaded_until_seq {
                return;
            }
            self.loaded_until_seq = event.seq;
            if let Some(session_id) = self.active_session_id
                && let Some(state) = self.sessions.get_mut(&session_id)
            {
                state.loaded_until_seq = event.seq;
            }
        }
        if event.kind == "generation_started" {
            let Some(generation_id) = event.generation_id else {
                return;
            };
            if self.status != ConversationStatus::Active
                || self.ignored_generation_ids.contains(&generation_id)
                || self.active_generation_id.is_some()
            {
                return;
            }
            self.active_generation_id = Some(generation_id);
            self.stream_base_len = Some(self.transcript.len());
            self.reasoning_buffer.clear();
        } else {
            let ignored_generation = event
                .generation_id
                .is_some_and(|generation_id| self.ignored_generation_ids.contains(&generation_id));
            let cancellation_completion = ignored_generation
                && event.kind == "generation_finished"
                && serde_json::from_str::<serde_json::Value>(&event.payload_json)
                    .ok()
                    .and_then(|payload| {
                        payload
                            .get("status")
                            .and_then(|v| v.as_str())
                            .map(|status| status == "cancelled")
                    })
                    .unwrap_or(false);
            let is_session_event =
                matches!(event.kind.as_str(), "prompt_admitted" | "prompt_promoted");
            if (ignored_generation && !cancellation_completion)
                || (!is_session_event
                    && self
                        .active_generation_id
                        .is_some_and(|active_generation_id| {
                            event.generation_id != Some(active_generation_id)
                        }))
            {
                return;
            }
        }
        match event.kind.as_str() {
            "finish" => {
                if let Ok(payload) = serde_json::from_str::<serde_json::Value>(&event.payload_json)
                    && let Some(reason) = payload.get("reason").and_then(|v| v.as_str())
                {
                    self.pending_finish_reason = parse_finish_reason(reason);
                }
            }
            "reasoning_delta" => {
                if let Ok(payload) = serde_json::from_str::<serde_json::Value>(&event.payload_json)
                    && let Some(delta) = payload.get("delta").and_then(|v| v.as_str())
                {
                    if self.reasoning_start.is_none() {
                        self.reasoning_start = Some(std::time::Instant::now());
                    }
                    self.reasoning_active = true;
                    self.push_stream_part(StreamPart::Reasoning(delta.to_string()));
                    self.reasoning_buffer.push_str(delta);
                }
            }
            "tool_call_start" => {
                self.text_stream_active = false;
                self.reasoning_active = false;
                self.record_reasoning_duration();
                if let Ok(payload) = serde_json::from_str::<serde_json::Value>(&event.payload_json)
                {
                    let name = payload
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("tool");
                    let call_id = payload
                        .get("id")
                        .and_then(|v| v.as_str())
                        .map(ToString::to_string)
                        .unwrap_or_else(|| format!("{name}:{}", event.seq));
                    self.push_tool_part(call_id.clone());
                    self.upsert_tool_row(
                        &call_id,
                        name,
                        ToolRowState::Pending,
                        "preparing arguments...".to_string(),
                        String::new(),
                        payload.get("metadata").cloned(),
                    );
                }
            }
            "tool_call_delta" => {
                if let Ok(payload) = serde_json::from_str::<serde_json::Value>(&event.payload_json)
                {
                    let call_id = payload.get("id").and_then(|v| v.as_str()).unwrap_or("");
                    let delta = payload
                        .get("arguments")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if let Some(row) = self.tool_rows.iter_mut().find(|row| row.call_id == call_id)
                    {
                        row.arguments = bounded(
                            format!("{}{}", row.arguments, delta),
                            MAX_TOOL_ARGUMENT_BYTES,
                        );
                        if let Ok(args) = serde_json::from_str::<serde_json::Value>(&row.arguments)
                        {
                            let (_, _, desc) = tool_target_and_verbs(&row.name, Some(&args));
                            if !desc.is_empty() {
                                row.desc = bounded(desc, MAX_TOOL_ARGUMENT_BYTES);
                            }
                        }
                        row.expandable = row.compute_expandable();
                    }
                    self.refresh_active_tool();
                }
            }
            "tool_call_end" => {
                let Ok(payload) = serde_json::from_str::<serde_json::Value>(&event.payload_json)
                else {
                    return;
                };
                let Some(call_id) = payload.get("id").and_then(|v| v.as_str()) else {
                    return;
                };
                if let Some(row) = self.tool_rows.iter_mut().find(|row| row.call_id == call_id) {
                    row.arguments_complete = true;
                    if let Some(metadata) = payload.get("metadata") {
                        row.metadata = Some(metadata.clone());
                    }
                    if row.desc == "preparing arguments..."
                        && let Ok(args) = serde_json::from_str::<serde_json::Value>(&row.arguments)
                    {
                        let (_, _, desc) = tool_target_and_verbs(&row.name, Some(&args));
                        if !desc.is_empty() {
                            row.desc = bounded(desc, MAX_TOOL_ARGUMENT_BYTES);
                        }
                    }
                    row.expandable = row.compute_expandable();
                }
            }
            "text_delta" => {
                self.reasoning_active = false;
                self.record_reasoning_duration();
                self.flush_reasoning();
                if let Ok(payload) = serde_json::from_str::<serde_json::Value>(&event.payload_json)
                    && let Some(delta) = payload.get("delta").and_then(|v| v.as_str())
                {
                    self.push_stream_part(StreamPart::Text(delta.to_string()));
                    if let Some(flushed) = self.typewriter.push_delta(delta) {
                        self.transcript.push_str(&flushed);
                        self.truncate_transcript();
                    }
                    self.text_stream_active = true;
                }
            }
            "tool_executing" => {
                self.text_stream_active = false;
                self.reasoning_active = false;
                self.record_reasoning_duration();
                if let Ok(payload) = serde_json::from_str::<serde_json::Value>(&event.payload_json)
                {
                    let name = payload
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("tool");
                    let args = payload.get("arguments");
                    let (_verb, _active_verb, mut desc) = tool_target_and_verbs(name, args);
                    if desc.is_empty()
                        && let Some(prev) = &self.active_tool
                        && prev.name == name
                        && !prev.desc.is_empty()
                    {
                        desc = prev.desc.clone();
                    }
                    if desc.is_empty() {
                        desc = "preparing arguments...".to_string();
                    }
                    let call_id = payload
                        .get("id")
                        .and_then(|v| v.as_str())
                        .map(ToString::to_string)
                        .unwrap_or_else(|| format!("inline:{}:{}", name, event.seq));
                    self.push_tool_part(call_id.clone());
                    let arguments = args.map(serde_json::Value::to_string).unwrap_or_default();
                    self.upsert_tool_row(
                        &call_id,
                        name,
                        ToolRowState::Running,
                        desc.clone(),
                        arguments,
                        payload.get("metadata").cloned(),
                    );
                    self.active_tool = Some(ActiveToolInfo {
                        name: name.to_string(),
                        desc,
                        started_at: std::time::Instant::now(),
                    });
                    if name == "question" {
                        let parsed_args_obj = if let Some(serde_json::Value::String(s)) = args {
                            serde_json::from_str::<serde_json::Value>(s).ok()
                        } else {
                            None
                        };
                        let effective_args = parsed_args_obj.as_ref().or(args);
                        let q_str = effective_args
                            .and_then(|a| a.get("question"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        let options = effective_args
                            .and_then(|a| a.get("options"))
                            .and_then(|v| v.as_array())
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|item| item.as_str().map(|s| s.to_string()))
                                    .collect::<Vec<String>>()
                            })
                            .unwrap_or_default();
                        self.open_question_dialog(q_str, options);
                    }
                }
            }
            "tool_executed" => {
                if let Ok(payload) = serde_json::from_str::<serde_json::Value>(&event.payload_json)
                {
                    let name = payload
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("tool");
                    let success = payload
                        .get("success")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false)
                        && payload.get("output").is_some();
                    let output = payload.get("output").and_then(|v| v.as_str()).unwrap_or("");
                    let args = payload.get("arguments");

                    let (_verb, _active_verb, target) = tool_target_and_verbs(name, args);
                    let call_id = payload.get("id").and_then(|v| v.as_str()).unwrap_or("");
                    let existing_id = if !call_id.is_empty() {
                        if self.tool_rows.iter().any(|row| row.call_id == call_id) {
                            Some(call_id.to_string())
                        } else {
                            self.tool_rows
                                .iter()
                                .rev()
                                .find(|row| {
                                    tool_names_match(&row.name, name)
                                        && matches!(
                                            row.state,
                                            ToolRowState::Pending | ToolRowState::Running
                                        )
                                })
                                .map(|row| row.call_id.clone())
                        }
                    } else {
                        self.tool_rows
                            .iter()
                            .rev()
                            .find(|row| {
                                tool_names_match(&row.name, name)
                                    && matches!(
                                        row.state,
                                        ToolRowState::Pending | ToolRowState::Running
                                    )
                            })
                            .or_else(|| {
                                self.tool_rows.iter().rev().find(|row| {
                                    matches!(
                                        row.state,
                                        ToolRowState::Pending | ToolRowState::Running
                                    )
                                })
                            })
                            .map(|row| row.call_id.clone())
                    };
                    let part_id = existing_id.clone().unwrap_or_else(|| {
                        if !call_id.is_empty() {
                            call_id.to_string()
                        } else {
                            format!("inline:{}:{}", name, event.seq)
                        }
                    });
                    let had_row = existing_id.is_some()
                        || (!call_id.is_empty()
                            && self.tool_rows.iter().any(|row| row.call_id == call_id));
                    if !had_row {
                        self.upsert_tool_row(
                            &part_id,
                            name,
                            ToolRowState::Running,
                            target.clone(),
                            args.map(serde_json::Value::to_string).unwrap_or_default(),
                            payload.get("metadata").cloned(),
                        );
                    } else if let Some(row) =
                        self.tool_rows.iter_mut().find(|row| row.call_id == part_id)
                    {
                        row.name = name.to_string();
                        if !target.is_empty()
                            && (row.desc == "preparing arguments..." || row.desc.is_empty())
                        {
                            row.desc = bounded(target.clone(), MAX_TOOL_ARGUMENT_BYTES);
                        }
                        if let Some(a) = args {
                            row.arguments = bounded(a.to_string(), MAX_TOOL_ARGUMENT_BYTES);
                        }
                        row.expandable = row.compute_expandable();
                    }
                    if name == "update_plan" && success {
                        self.current_plan = parse_plan_items(args);
                    }
                    self.complete_tool_row(&part_id, name, success, output);
                    self.push_tool_part(part_id);
                    self.refresh_active_tool();
                }
            }
            "generation_finished" => {
                self.flush_typewriter();
                self.flush_reasoning();
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
                    if self.typewriter.is_typing() {
                        self.typewriter.set_pending_status(target_status);
                    } else {
                        self.typewriter.reset();
                        self.status = target_status;
                    }
                }
            }
            "error" => {
                self.flush_reasoning();
                self.flush_typewriter();
                self.text_stream_active = false;
                for row in &mut self.tool_rows {
                    if matches!(row.state, ToolRowState::Pending | ToolRowState::Running) {
                        row.state = ToolRowState::Failed;
                    }
                }
                self.refresh_active_tool();
                if let Ok(payload) = serde_json::from_str::<serde_json::Value>(&event.payload_json)
                    && let Some(message) = payload.get("message").and_then(|v| v.as_str())
                {
                    self.diagnostic = bounded(message.to_string(), MAX_DIAGNOSTIC_BYTES);
                    self.status = ConversationStatus::Error;
                }
            }
            "prompt_admitted" => {
                tracing::debug!(
                    session_id = event.session_id,
                    seq = event.seq,
                    "prompt admitted"
                );
            }
            "prompt_promoted" => {
                tracing::debug!(
                    session_id = event.session_id,
                    seq = event.seq,
                    "prompt promoted"
                );
            }
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
                            self.flush_reasoning();
                            self.flush_typewriter();
                            if !self.transcript.contains(trimmed) {
                                if !self.transcript.is_empty() && !self.transcript.ends_with("\n\n")
                                {
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
            _ => {}
        }
    }

    /// Drain pending runtime events into the live transcript view.
    /// The bounded bus applies backpressure and prunes closed receivers;
    /// this drains pending events to exhaustion each call.
    pub fn poll_runtime(&mut self) -> bool {
        self.command_service.poll_refresh();
        if let Ok(models) = self.command_service.models()
            && !models.is_empty()
            && models != self.available_models
        {
            self.available_models = models;
        }
        let Some(receiver) = self.runtime_events.take() else {
            return false;
        };
        let mut had_events = false;
        while let Ok(event) = receiver.try_recv() {
            had_events = true;
            self.handle_runtime_event(event);
        }
        // Advance typewriter drain so headless loops and fast polls drain
        if self.typewriter.is_typing() || self.typewriter.pending_status().is_some() {
            if let Some(chunk) = self.typewriter.drain_step() {
                self.transcript.push_str(&chunk);
                self.truncate_transcript();
                had_events = true;
            }
            if !self.typewriter.is_typing()
                && let Some(target) = self.typewriter.take_pending_status()
            {
                self.typewriter.reset();
                self.status = target;
                had_events = true;
            }
        }
        self.runtime_events = Some(receiver);
        had_events
    }
}
