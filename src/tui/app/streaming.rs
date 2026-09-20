use super::tool_rows::ToolRowState;
use super::util::{ceil_char_boundary, find_user_turn_boundary, floor_char_boundary};
use super::{App, ConversationStatus};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamPart {
    User(String),
    Text(String),
    Reasoning(String),
    Tool(String),
}

impl App {
    pub(crate) fn reset_turn_view(&mut self, clear_transcript: bool) {
        self.flush_typewriter();
        self.flush_reasoning();
        if clear_transcript {
            self.transcript.clear();
            self.current_plan.clear();
            self.stream_parts.clear();
            self.tool_rows.clear();
            self.expanded_tool_rows.clear();
        }
        for row in &mut self.tool_rows {
            if matches!(row.state, ToolRowState::Pending | ToolRowState::Running) {
                row.state = ToolRowState::Failed;
            }
        }
        self.stream_base_len = None;
        self.typewriter.reset();
        self.text_stream_active = false;
        self.reasoning_buffer.clear();
        self.reasoning_start = None;
        self.reasoning_duration = None;
        self.reasoning_active = false;
        self.active_tool = None;
        if let Some(generation_id) = self.active_generation_id.take() {
            self.ignored_generation_ids.insert(generation_id);
        }
        self.thought_expanded = false;
        self.chat_scroll = 0;
    }

    /// Whether assistant text is visible or may resume during active generation.
    pub fn is_typing(&self) -> bool {
        self.typewriter.is_typing()
            || (self.text_stream_active
                && matches!(self.status, ConversationStatus::Active)
                && !self.is_reasoning()
                && !self
                    .tool_rows
                    .iter()
                    .any(|row| matches!(row.state, ToolRowState::Pending | ToolRowState::Running)))
    }

    /// Whether generation, text, reasoning, or any tool row is active.
    pub fn is_streaming_active(&self) -> bool {
        matches!(self.status, ConversationStatus::Active)
            || self.typewriter.is_typing()
            || self.typewriter.pending_status().is_some()
            || self.is_reasoning()
            || self
                .tool_rows
                .iter()
                .any(|row| matches!(row.state, ToolRowState::Pending | ToolRowState::Running))
    }

    pub fn reasoning_buffer(&self) -> &str {
        &self.reasoning_buffer
    }

    pub fn is_reasoning(&self) -> bool {
        self.reasoning_active && self.active_tool.is_none()
    }

    pub fn reasoning_elapsed_seconds(&self) -> Option<f64> {
        self.reasoning_duration
            .map(|d| d.as_secs_f64())
            .or_else(|| self.reasoning_start.map(|t| t.elapsed().as_secs_f64()))
    }

    pub(crate) fn record_reasoning_duration(&mut self) {
        if let Some(start) = self.reasoning_start
            && self.reasoning_duration.is_none()
        {
            self.reasoning_duration = Some(start.elapsed());
        }
    }

    pub fn flush_reasoning(&mut self) {
        if self.reasoning_buffer.trim().is_empty() {
            self.reasoning_buffer.clear();
            self.reasoning_start = None;
            self.reasoning_duration = None;
            self.reasoning_active = false;
            return;
        }
        let duration = self
            .reasoning_duration
            .or_else(|| self.reasoning_start.map(|t| t.elapsed()))
            .unwrap_or_default()
            .as_secs_f64();
        let dur_str = format!("{:.1}s", duration.max(0.1));

        let mut snippet = String::new();
        if !self.transcript.is_empty() && !self.transcript.ends_with('\n') {
            snippet.push('\n');
        }
        if !self.transcript.is_empty() && !self.transcript.ends_with("\n\n") {
            snippet.push('\n');
        }
        snippet.push_str(&format!("Thought for {dur_str}\n\n"));
        self.transcript.push_str(&snippet);
        self.truncate_transcript();
        self.reasoning_buffer.clear();
        self.reasoning_start = None;
        self.reasoning_duration = None;
        self.reasoning_active = false;
    }

    pub fn stream_parts(&self) -> &[StreamPart] {
        &self.stream_parts
    }

    pub fn stream_base_len(&self) -> Option<usize> {
        self.stream_base_len
    }

    pub fn set_stream_parts_for_test(&mut self, parts: Vec<StreamPart>, base_len: Option<usize>) {
        self.stream_parts = parts;
        self.stream_base_len = base_len;
    }

    pub(crate) fn ensure_stream_parts(&mut self) {
        if self.stream_base_len.is_none() {
            self.stream_base_len = Some(self.transcript.len());
        }
    }

    pub(crate) fn push_stream_part(&mut self, part: StreamPart) {
        self.ensure_stream_parts();
        match part {
            StreamPart::User(prompt) => {
                self.stream_parts.push(StreamPart::User(prompt));
            }
            StreamPart::Reasoning(delta) => {
                let last_user_idx = self
                    .stream_parts
                    .iter()
                    .rposition(|p| matches!(p, StreamPart::User(_)))
                    .unwrap_or(0);
                if let Some(StreamPart::Reasoning(existing)) = self.stream_parts[last_user_idx..]
                    .iter_mut()
                    .find(|p| matches!(p, StreamPart::Reasoning(_)))
                {
                    existing.push_str(&delta);
                    if existing.len() > Self::MAX_TRANSCRIPT_BYTES {
                        existing
                            .truncate(floor_char_boundary(existing, Self::MAX_TRANSCRIPT_BYTES));
                    }
                } else {
                    self.stream_parts.push(StreamPart::Reasoning(delta));
                }
            }
            StreamPart::Text(delta) => {
                if let Some(StreamPart::Text(existing)) = self.stream_parts.last_mut() {
                    existing.push_str(&delta);
                    if existing.len() > Self::MAX_TRANSCRIPT_BYTES {
                        existing
                            .truncate(floor_char_boundary(existing, Self::MAX_TRANSCRIPT_BYTES));
                    }
                } else {
                    self.stream_parts.push(StreamPart::Text(delta));
                }
            }
            StreamPart::Tool(call_id) => {
                if !self
                    .stream_parts
                    .iter()
                    .any(|part| matches!(part, StreamPart::Tool(id) if id == &call_id))
                {
                    self.stream_parts.push(StreamPart::Tool(call_id.clone()));
                }
                if let Some(row) = self.tool_rows.iter_mut().find(|r| r.call_id == call_id) {
                    row.expandable = row.compute_expandable();
                }
            }
        }
        if self.stream_parts.len() > 1024 {
            self.stream_parts.drain(..self.stream_parts.len() - 1024);
        }
    }

    pub(crate) fn push_tool_part(&mut self, call_id: String) {
        self.ensure_stream_parts();
        if !self
            .stream_parts
            .iter()
            .any(|part| matches!(part, StreamPart::Tool(id) if id == &call_id))
        {
            self.stream_parts.push(StreamPart::Tool(call_id.clone()));
        }
        if let Some(row) = self.tool_rows.iter_mut().find(|r| r.call_id == call_id) {
            row.expandable = row.compute_expandable();
        }
    }

    pub fn wave_spinner(&self) -> &crate::tui::WaveSpinner {
        &self.wave_spinner
    }

    pub fn wave_spinner_mut(&mut self) -> &mut crate::tui::WaveSpinner {
        &mut self.wave_spinner
    }

    pub fn typewriter(&self) -> &crate::tui::TypewriterState {
        &self.typewriter
    }

    pub fn typewriter_mut(&mut self) -> &mut crate::tui::TypewriterState {
        &mut self.typewriter
    }

    pub fn tokens_per_second(&self) -> Option<f64> {
        self.typewriter.tokens_per_second()
    }

    pub fn streaming_elapsed_seconds(&self) -> Option<f64> {
        self.typewriter.elapsed_seconds()
    }

    /// Flush all pending typewriter characters directly into the transcript.
    pub fn flush_typewriter(&mut self) {
        let remaining = self.typewriter.flush();
        if !remaining.is_empty() {
            self.transcript.push_str(&remaining);
            self.truncate_transcript();
        }
        if let Some(target) = self.typewriter.take_pending_status() {
            self.status = target;
        }
    }

    pub fn chat_scroll(&self) -> u16 {
        self.chat_scroll
    }

    pub fn scroll_up(&mut self, lines: u16) {
        self.chat_scroll = self.chat_scroll.saturating_add(lines);
    }

    pub fn scroll_down(&mut self, lines: u16) {
        self.chat_scroll = self.chat_scroll.saturating_sub(lines);
    }

    pub fn scroll_to_top(&mut self) {
        self.chat_scroll = u16::MAX;
    }

    pub fn scroll_to_bottom(&mut self) {
        self.chat_scroll = 0;
    }

    pub fn transcript(&self) -> &str {
        &self.transcript
    }

    pub fn current_plan(&self) -> &[(String, String)] {
        &self.current_plan
    }

    pub(crate) fn truncate_transcript(&mut self) {
        if self.transcript.len() <= Self::MAX_TRANSCRIPT_BYTES + 16 * 1024 {
            return;
        }

        let retained_bytes =
            Self::MAX_TRANSCRIPT_BYTES.saturating_sub(Self::TRUNCATION_MARKER.len());
        let cut_point = self.transcript.len().saturating_sub(retained_bytes);
        let start = find_user_turn_boundary(&self.transcript, cut_point)
            .unwrap_or_else(|| ceil_char_boundary(&self.transcript, cut_point));

        let len_before = self.transcript.len();
        self.transcript
            .replace_range(..start, Self::TRUNCATION_MARKER);
        let removed_bytes = len_before.saturating_sub(self.transcript.len());
        if let Some(base) = self.stream_base_len {
            self.stream_base_len = Some(base.saturating_sub(removed_bytes));
        }
    }
}
