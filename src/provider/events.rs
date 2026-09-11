//! Normalized stream events shared by every provider adapter. Adapters map
//! wire formats onto these variants; core never sees vendor payloads.

/// Why a stream finished. Normalized across providers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FinishReason {
    Stop,
    Length,
    ToolCall,
    Error,
}

/// Token usage reported at stream end.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

/// Maximum accumulated tool-call argument bytes per call id.
pub const MAX_TOOL_ARGUMENT_BYTES: usize = 256 * 1024;

/// Normalized streaming event. `Cancelled` is terminal: nothing may follow.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StreamEvent {
    TextDelta(String),
    ReasoningDelta(String),
    ToolCallStart { id: String, name: String },
    ToolCallDelta { id: String, arguments: String },
    ToolCallEnd { id: String },
    ToolResult { id: String, result: String },
    Usage(Usage),
    Finish { reason: FinishReason },
    Error(String),
    Cancelled,
}

/// Incremental tool-argument assembler with a hard size cap. Partial JSON
/// fragments accumulate per call id; `finish` returns the joined payload.
#[derive(Debug, Default)]
pub struct ToolCallAssembler {
    buffers: Vec<(String, String)>,
    cap: usize,
}

impl ToolCallAssembler {
    pub fn new(cap: usize) -> Self {
        Self {
            buffers: Vec::new(),
            cap,
        }
    }

    /// Append a fragment. Fails once the accumulated size exceeds the cap.
    pub fn push(&mut self, id: &str, fragment: &str) -> Result<(), ToolArgumentTooLarge> {
        let entry = match self.buffers.iter_mut().find(|(existing, _)| existing == id) {
            Some(entry) => entry,
            None => {
                self.buffers.push((id.to_owned(), String::new()));
                self.buffers.last_mut().expect("entry pushed above")
            }
        };
        if entry.1.len() + fragment.len() > self.cap {
            return Err(ToolArgumentTooLarge {
                id: id.to_owned(),
                cap: self.cap,
            });
        }
        entry.1.push_str(fragment);
        Ok(())
    }

    /// Take the accumulated arguments for `id`, if any.
    pub fn finish(&mut self, id: &str) -> Option<String> {
        let index = self
            .buffers
            .iter()
            .position(|(existing, _)| existing == id)?;
        Some(self.buffers.remove(index).1)
    }
}

/// Arguments for one tool call exceeded the configured cap.
#[derive(Debug)]
pub struct ToolArgumentTooLarge {
    pub id: String,
    pub cap: usize,
}

impl std::fmt::Display for ToolArgumentTooLarge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "tool call `{}` arguments exceed {} bytes",
            self.id, self.cap
        )
    }
}

impl std::error::Error for ToolArgumentTooLarge {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assembler_reuses_buffer_per_id() {
        let mut assembler = ToolCallAssembler::new(64);
        assembler.push("a", "{\"x\":").unwrap();
        assembler.push("b", "{\"y\":1}").unwrap();
        assembler.push("a", "1}").unwrap();
        assert_eq!(assembler.finish("a").unwrap(), "{\"x\":1}");
        assert_eq!(assembler.finish("b").unwrap(), "{\"y\":1}");
        assert!(assembler.finish("a").is_none());
    }
}
