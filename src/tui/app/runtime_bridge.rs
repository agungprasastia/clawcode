use super::tool_rows::{
    ActiveToolInfo, ToolRowState, ToolRowUpdate, tool_names_match, tool_target_and_verbs,
};
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
        self.attach_runtime_with_mcp(db, writer, provider, None);
    }

    /// Attach a runtime with MCP servers from the resolved configuration.
    pub fn attach_runtime_with_mcp(
        &mut self,
        db: crate::persistence::Db,
        writer: crate::persistence::WriterHandle,
        provider: Box<dyn crate::provider::Provider + Send + Sync>,
        mcp: Option<std::sync::Arc<crate::mcp::McpRegistry>>,
    ) {
        let bus = EventBus::new();
        let (_subscription_id, receiver) = bus.subscribe(None);
        self.mcp_registry = mcp.clone();
        let client = RuntimeClient::spawn_with_mcp(db, writer, provider, bus, mcp);
        self.runtime_events = Some(receiver);
        self.runtime = Some(client);
        self.hydrate_kv_state();
        self.start_mcp_startup_probes();
    }

    pub fn set_runtime_receiver(&mut self, receiver: std::sync::mpsc::Receiver<RuntimeEvent>) {
        self.runtime_events = Some(receiver);
    }
    pub fn runtime(&self) -> Option<&RuntimeClient> {
        self.runtime.as_ref()
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
            if let Some(state) = self.sessions.get_mut(&event.session_id)
                && let Ok(payload) = serde_json::from_str::<serde_json::Value>(&event.payload_json)
                && let Some(input_id) = payload.get("input_id").and_then(|v| v.as_i64())
            {
                match event.kind.as_str() {
                    "prompt_updated" => {
                        if let Some(delivery) = payload.get("delivery").and_then(|v| v.as_str())
                            && let Some(item) =
                                state.pending_prompts.iter_mut().find(|p| p.id == input_id)
                        {
                            item.delivery = delivery.to_string();
                        }
                    }
                    "prompt_discarded" | "prompt_promoted" => {
                        state.pending_prompts.retain(|p| p.id != input_id);
                    }
                    _ => {}
                }
            }
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
            let is_session_event = matches!(
                event.kind.as_str(),
                "prompt_admitted"
                    | "prompt_promoted"
                    | "prompt_updated"
                    | "prompt_discarded"
                    | "tool_background_finished"
            );
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
                    self.push_stream_part(StreamPart::Reasoning(delta.to_string()));
                    if self.reasoning_start.is_none() {
                        self.reasoning_start = Some(std::time::Instant::now());
                    }
                    self.reasoning_active = true;
                    self.reasoning_buffer.push_str(delta);
                }
            }
            "tool_call_start" => {
                self.text_stream_active = false;
                self.flush_reasoning();
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
                        .unwrap_or_else(|| {
                            format!("{name}:{}:{}", event.seq, self.tool_rows.len())
                        });
                    self.push_tool_part(call_id.clone());
                    self.upsert_tool_row(ToolRowUpdate {
                        call_id: &call_id,
                        name,
                        state: ToolRowState::Pending,
                        desc: "preparing arguments...".to_string(),
                        arguments: String::new(),
                        metadata: payload.get("metadata").cloned(),
                    });
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
                        // Parsing partial JSON is wasted work on large
                        // arguments; only retry when the value looks closed
                        // or the payload is still small.
                        let looks_complete = row.arguments.len() <= 4096
                            || row.arguments.trim_end().ends_with(['}', ']']);
                        if looks_complete
                            && let Ok(args) =
                                serde_json::from_str::<serde_json::Value>(&row.arguments)
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
                self.flush_reasoning();
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
                        .unwrap_or_else(|| {
                            format!("inline:{}:{}:{}", name, event.seq, self.tool_rows.len())
                        });
                    self.push_tool_part(call_id.clone());
                    let arguments = args.map(serde_json::Value::to_string).unwrap_or_default();
                    self.upsert_tool_row(ToolRowUpdate {
                        call_id: &call_id,
                        name,
                        state: ToolRowState::Running,
                        desc: desc.clone(),
                        arguments,
                        metadata: payload.get("metadata").cloned(),
                    });
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
            "tool_call_progress" => {
                if let Ok(payload) = serde_json::from_str::<serde_json::Value>(&event.payload_json)
                {
                    let call_id = payload.get("id").and_then(|v| v.as_str()).unwrap_or("");
                    let output = payload.get("output").and_then(|v| v.as_str()).unwrap_or("");
                    self.append_tool_progress(call_id, output);
                }
            }
            "tool_call_created" | "tool_call_started" => {
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
                    if self.tool_rows.iter().any(|row| row.call_id == call_id) {
                        // The volatile `tool_executing` event usually arrives
                        // first. Durable lifecycle events must enrich that
                        // row without stepping it backwards to Pending.
                        if let Some(row) =
                            self.tool_rows.iter_mut().find(|row| row.call_id == call_id)
                        {
                            row.name = tool_name.to_string();
                            if !arguments.is_empty() {
                                row.arguments = bounded(arguments, MAX_TOOL_ARGUMENT_BYTES);
                            }
                            if !desc.is_empty() {
                                row.desc = bounded(desc, MAX_TOOL_ARGUMENT_BYTES);
                            }
                            if event.kind == "tool_call_started"
                                && row.state == ToolRowState::Pending
                            {
                                row.state = ToolRowState::Running;
                            }
                            if let Some(metadata) = payload.get("metadata") {
                                row.metadata = Some(metadata.clone());
                            }
                            row.expandable = row.compute_expandable();
                        }
                    } else {
                        self.upsert_tool_row(ToolRowUpdate {
                            call_id,
                            name: tool_name,
                            state: if event.kind == "tool_call_started" {
                                ToolRowState::Running
                            } else {
                                ToolRowState::Pending
                            },
                            desc,
                            arguments,
                            metadata: payload.get("metadata").cloned(),
                        });
                    }
                    self.push_tool_part(call_id.to_string());
                    self.refresh_active_tool();
                }
            }
            "tool_call_settled" | "assistant_message_updated" => {
                self.apply_replayed_event(&event);
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
                        self.upsert_tool_row(ToolRowUpdate {
                            call_id: &part_id,
                            name,
                            state: ToolRowState::Running,
                            desc: target.clone(),
                            arguments: args.map(serde_json::Value::to_string).unwrap_or_default(),
                            metadata: payload.get("metadata").cloned(),
                        });
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
                let payload = serde_json::from_str::<serde_json::Value>(&event.payload_json)
                    .unwrap_or_else(|_| serde_json::json!({}));
                let status = payload
                    .get("status")
                    .and_then(|v| v.as_str())
                    .unwrap_or("completed");
                self.text_stream_active = false;
                let finish_reason = payload
                    .get("finish_reason")
                    .and_then(|v| v.as_str())
                    .and_then(parse_finish_reason)
                    .or_else(|| self.pending_finish_reason.take());
                let target_status = match status {
                    "cancelled" => ConversationStatus::Cancelled,
                    "failed" => ConversationStatus::Error,
                    _ => ConversationStatus::Finished(finish_reason.unwrap_or(FinishReason::Stop)),
                };
                if self.typewriter.is_typing() {
                    self.typewriter.set_pending_status(target_status);
                } else {
                    self.typewriter.reset();
                    self.status = target_status;
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
            "prompt_updated" => {
                if let Ok(payload) = serde_json::from_str::<serde_json::Value>(&event.payload_json)
                    && let Some(input_id) = payload.get("input_id").and_then(|v| v.as_i64())
                    && let Some(delivery) = payload.get("delivery").and_then(|v| v.as_str())
                    && let Some(pos) = self.pending_prompts.iter().position(|p| p.id == input_id)
                {
                    let mut item = self.pending_prompts.remove(pos);
                    item.delivery = delivery.to_string();
                    if delivery == "steer" {
                        let insert_at = self
                            .pending_prompts
                            .iter()
                            .position(|p| p.delivery != "steer")
                            .unwrap_or(self.pending_prompts.len());
                        self.pending_prompts.insert(insert_at, item);
                    } else {
                        self.pending_prompts.push(item);
                    }
                    if let Some(dialog) = &mut self.queued_prompts_dialog {
                        dialog.items = self.pending_prompts.clone();
                    }
                }
            }
            "prompt_discarded" => {
                if let Ok(payload) = serde_json::from_str::<serde_json::Value>(&event.payload_json)
                    && let Some(input_id) = payload.get("input_id").and_then(|v| v.as_i64())
                {
                    self.pending_prompts.retain(|p| p.id != input_id);
                    if let Some(dialog) = &mut self.queued_prompts_dialog {
                        dialog.items = self.pending_prompts.clone();
                    }
                }
            }
            "prompt_promoted" => {
                tracing::debug!(
                    session_id = event.session_id,
                    seq = event.seq,
                    "prompt promoted"
                );
                if let Ok(payload) = serde_json::from_str::<serde_json::Value>(&event.payload_json)
                    && let Some(input_id) = payload.get("input_id").and_then(|v| v.as_i64())
                    && let Some(pos) = self.pending_prompts.iter().position(|p| p.id == input_id)
                {
                    let promoted = self.pending_prompts.remove(pos);
                    if let Some(dialog) = &mut self.queued_prompts_dialog {
                        dialog.items = self.pending_prompts.clone();
                        if dialog.items.is_empty() {
                            self.queued_prompts_dialog = None;
                        }
                    }
                    self.reset_turn_view(false);
                    let prompt = promoted.content;
                    if self.transcript.is_empty() {
                        self.transcript.push_str(&format!("> {prompt}\n\n"));
                    } else {
                        self.transcript.push_str(&format!("\n\n> {prompt}\n\n"));
                    }
                    self.truncate_transcript();
                    self.push_stream_part(StreamPart::User(prompt.clone()));
                    self.stream_base_len = Some(self.transcript.len());
                    let provider = if self.provider.is_empty() {
                        "anthropic".to_string()
                    } else {
                        self.provider.clone()
                    };
                    let model = if self.model.is_empty() {
                        "claude-3-7-sonnet".to_string()
                    } else {
                        self.model.clone()
                    };
                    self.apply_conversation(
                        crate::conversation::ConversationEvent::PromptSubmitted {
                            prompt,
                            provider,
                            model,
                        },
                    );
                }
            }
            "tool_background_finished" => {
                if let Ok(payload) = serde_json::from_str::<serde_json::Value>(&event.payload_json)
                {
                    let call_id = payload.get("id").and_then(|v| v.as_str()).unwrap_or("");
                    let name = payload
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("bash");
                    let success = payload
                        .get("success")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    let output = payload.get("output").and_then(|v| v.as_str()).unwrap_or("");
                    if let Some(row) = self.tool_rows.iter_mut().find(|r| r.call_id == call_id) {
                        let mut meta = row
                            .metadata
                            .take()
                            .and_then(|v| v.as_object().cloned())
                            .unwrap_or_default();
                        meta.insert("background".to_string(), serde_json::Value::Bool(true));
                        meta.insert(
                            "status".to_string(),
                            serde_json::Value::String(
                                if success { "completed" } else { "failed" }.to_string(),
                            ),
                        );
                        row.metadata = Some(serde_json::Value::Object(meta));
                        row.state = if success {
                            ToolRowState::Completed
                        } else {
                            ToolRowState::Failed
                        };
                        if !output.is_empty() {
                            row.output = bounded(output.to_string(), super::MAX_TOOL_OUTPUT_BYTES);
                        }
                        row.expandable = row.compute_expandable();
                    }
                    self.diagnostic = if success {
                        format!("background shell finished: {name}")
                    } else {
                        format!("background shell failed: {name}")
                    };
                }
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
                                if self.chat_scroll == 0 {
                                    self.scroll_to_bottom();
                                }
                            }
                        }
                    }
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
        let mut had_events = self.poll_mcp_probes() || self.terminal_pane.poll();
        let Some(receiver) = self.runtime_events.take() else {
            return had_events;
        };
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
