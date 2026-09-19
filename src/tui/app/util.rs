use std::collections::VecDeque;

use super::{Input, UiEvent};

pub(crate) fn bounded(mut value: String, limit: usize) -> String {
    let end = floor_char_boundary(&value, limit.min(value.len()));
    value.truncate(end);
    value
}

#[derive(Debug)]
pub struct UiEventQueue {
    capacity: usize,
    quit: bool,
    cancel: bool,
    events: VecDeque<UiEvent>,
    delta: String,
}

impl UiEventQueue {
    pub const MAX_COALESCED_STREAM_BYTES: usize = 64 * 1024;

    pub fn new(capacity: usize) -> Self {
        let capacity = capacity.max(3);
        Self {
            capacity,
            quit: false,
            cancel: false,
            events: VecDeque::with_capacity(capacity.saturating_sub(3)),
            delta: String::new(),
        }
    }

    pub fn push(&mut self, event: UiEvent) {
        match event {
            UiEvent::Input(Input::Quit) => self.quit = true,
            UiEvent::Input(Input::Cancel) => self.cancel = true,
            UiEvent::StreamDelta(delta) => {
                let remaining = Self::MAX_COALESCED_STREAM_BYTES.saturating_sub(self.delta.len());
                let end = floor_char_boundary(&delta, remaining.min(delta.len()));
                self.delta.push_str(&delta[..end]);
            }
            event => {
                let max_events = self.capacity.saturating_sub(3);
                while self.events.len() >= max_events && !self.events.is_empty() {
                    self.events.pop_front();
                }
                if max_events > 0 {
                    self.events.push_back(event);
                }
            }
        }
    }

    pub fn len(&self) -> usize {
        usize::from(self.quit)
            + usize::from(self.cancel)
            + self.events.len()
            + usize::from(!self.delta.is_empty())
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub(crate) fn pop_priority(&mut self) -> Option<UiEvent> {
        if std::mem::take(&mut self.quit) {
            Some(UiEvent::Input(Input::Quit))
        } else if std::mem::take(&mut self.cancel) {
            Some(UiEvent::Input(Input::Cancel))
        } else {
            None
        }
    }

    pub(crate) fn drain(&mut self) -> Vec<UiEvent> {
        let mut drained =
            Vec::with_capacity(self.events.len() + usize::from(!self.delta.is_empty()));
        drained.extend(self.events.drain(..));
        if !self.delta.is_empty() {
            drained.push(UiEvent::StreamDelta(std::mem::take(&mut self.delta)));
        }
        drained
    }

    #[allow(dead_code)]
    pub(crate) fn clear(&mut self) {
        self.quit = false;
        self.cancel = false;
        self.events.clear();
        self.delta.clear();
    }
}

pub(crate) fn find_user_turn_boundary(value: &str, cutoff: usize) -> Option<usize> {
    let cutoff = floor_char_boundary(value, cutoff);
    let mut in_code_block = false;
    for line in value[..cutoff].split_inclusive('\n') {
        if line.trim_start().starts_with("```") {
            in_code_block = !in_code_block;
        }
    }

    let tail = &value[cutoff..];
    let mut offset = 0;
    for line in tail.split_inclusive('\n') {
        let start = cutoff + offset;
        let at_line_start = start == 0 || value.as_bytes()[start - 1] == b'\n';
        if at_line_start && !in_code_block && line.starts_with("> ") {
            return Some(start);
        }
        if line.trim_start().starts_with("```") {
            in_code_block = !in_code_block;
        }
        offset += line.len();
    }
    None
}

pub(crate) fn floor_char_boundary(value: &str, index: usize) -> usize {
    let mut index = index.min(value.len());
    while !value.is_char_boundary(index) {
        index = index.saturating_sub(1);
    }
    index
}

pub(crate) fn ceil_char_boundary(value: &str, mut index: usize) -> usize {
    if index >= value.len() {
        return value.len();
    }
    while !value.is_char_boundary(index) {
        index += 1;
    }
    index
}

pub const PLACEHOLDER_SUGGESTIONS: &[&str] = &[
    "Fix a TODO in the codebase",
    "What is the tech stack of this project?",
    "Write unit tests for this module",
    "Refactor this function for better performance",
    "Add error handling to this code",
    "Explain how this code works",
    "Find and fix a bug in this module",
    "Add documentation to this function",
    "Create a new feature for X",
    "Optimize this database query",
    "Add type hints to this code",
    "Implement caching for this endpoint",
];

pub fn get_random_placeholder() -> String {
    let index = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| (d.as_millis() as usize) % PLACEHOLDER_SUGGESTIONS.len())
        .unwrap_or(0);
    format!("Ask anything... \"{}\"", PLACEHOLDER_SUGGESTIONS[index])
}

pub fn split_provider_model(id: &str) -> Option<(&str, &str)> {
    if let Some((p, m)) = id.split_once('/')
        && matches!(
            p,
            "openai"
                | "anthropic"
                | "ollama"
                | "9router"
                | "groq"
                | "deepseek"
                | "gemini"
                | "openrouter"
                | "together"
        )
    {
        return Some((p, m));
    }
    None
}

pub fn is_sensitive_command(cmd: &str) -> bool {
    let lower = cmd.trim().to_lowercase();
    const PATTERNS: &[&str] = &[
        "rm ",
        "rm\t",
        "git reset",
        "git clean",
        "mkfs",
        "dd ",
        "dd\t",
        "kill ",
        "kill\t",
        "chmod ",
        "chmod\t",
        "chown ",
        "chown\t",
    ];
    if lower == "rm" || lower == "dd" || lower == "kill" || lower == "mkfs" {
        return true;
    }
    PATTERNS.iter().any(|&p| lower.contains(p))
}

pub(crate) fn parse_plan_items(args: Option<&serde_json::Value>) -> Vec<(String, String)> {
    let parsed_args_holder: Option<serde_json::Value> = match args {
        Some(serde_json::Value::String(s)) => serde_json::from_str(s).ok(),
        Some(v @ serde_json::Value::Object(_)) => Some(v.clone()),
        _ => None,
    };
    let args_ref = parsed_args_holder.as_ref().or(args);
    args_ref
        .and_then(|a| a.get("plan"))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| {
                    let obj = item.as_object()?;
                    let step = obj
                        .get("step")
                        .or_else(|| obj.get("content"))
                        .or_else(|| obj.get("title"))
                        .and_then(|v| v.as_str())?
                        .trim();
                    if step.is_empty() {
                        return None;
                    }
                    let clean_step = if let Some((num, rest)) = step.split_once(". ") {
                        if !num.is_empty() && num.chars().all(|c| c.is_ascii_digit()) {
                            rest.trim()
                        } else {
                            step
                        }
                    } else {
                        step
                    };

                    let status = obj
                        .get("status")
                        .and_then(|v| v.as_str())
                        .unwrap_or("pending")
                        .trim()
                        .to_ascii_lowercase();
                    let norm_status = match status.as_str() {
                        "todo" | "open" | "pending" | "not_started" | "not-started" => "pending",
                        "in_progress" | "in-progress" | "in progress" | "doing" | "active" => {
                            "in_progress"
                        }
                        "done" | "completed" | "complete" => "completed",
                        other => other,
                    };
                    Some((clean_step.to_string(), norm_status.to_string()))
                })
                .collect()
        })
        .unwrap_or_default()
}
