//! Protocol adapters. Wire parsing stays here; provider core sees normalized events.

use crate::provider::{
    FinishReason, ModelInfo, Provider, ProviderCapabilities, ProviderError, ProviderId,
    StreamEvent, StreamRequest, StreamResponse,
};
use serde_json::Value;
use std::time::Duration;

pub mod anthropic;
pub mod ollama;
pub mod openai_compatible;

pub trait Transport: std::fmt::Debug + Send + Sync {
    fn request(&self, endpoint: &str, body: &str) -> Result<String, ProviderError>;

    fn stream_request(
        &self,
        endpoint: &str,
        body: &str,
        sender: &crate::provider::StreamSender,
        mut parser: Box<dyn FnMut(Value) -> Result<Vec<StreamEvent>, ProviderError> + Send>,
    ) -> Result<(), ProviderError> {
        let response = self.request(endpoint, body)?;
        let parsed = parse_lines(&response, &mut *parser)?;
        for event in parsed.events {
            let _ = sender.send(event);
        }
        let _ = sender.flush();
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct MockTransport(pub String);
impl Transport for MockTransport {
    fn request(&self, _endpoint: &str, _body: &str) -> Result<String, ProviderError> {
        Ok(self.0.clone())
    }
}

#[derive(Debug, Clone)]
pub struct HttpTransport {
    pub api_key: Option<String>,
    pub headers: Vec<(String, String)>,
    pub timeout: Duration,
}

impl Default for HttpTransport {
    fn default() -> Self {
        Self {
            api_key: None,
            headers: Vec::new(),
            timeout: Duration::from_secs(60),
        }
    }
}

impl HttpTransport {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_api_key(mut self, key: impl Into<String>) -> Self {
        self.api_key = Some(key.into());
        self
    }

    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

impl Transport for HttpTransport {
    fn request(&self, endpoint: &str, body: &str) -> Result<String, ProviderError> {
        let mut req = ureq::post(endpoint)
            .timeout(self.timeout)
            .set("Content-Type", "application/json");

        if let Some(ref key) = self.api_key
            && !key.is_empty() {
                if endpoint.contains("anthropic") {
                    req = req
                        .set("x-api-key", key)
                        .set("anthropic-version", "2023-06-01");
                }
                req = req.set("Authorization", &format!("Bearer {key}"));
            }
        for (k, v) in &self.headers {
            req = req.set(k, v);
        }

        let resp = req.send_string(body).map_err(|e| match e {
            ureq::Error::Status(code, resp) => {
                let text = resp.into_string().unwrap_or_default();
                ProviderError::Network(format!("HTTP {code}: {text}"))
            }
            ureq::Error::Transport(e) => ProviderError::Network(e.to_string()),
        })?;

        resp.into_string()
            .map_err(|e| ProviderError::Network(e.to_string()))
    }

    fn stream_request(
        &self,
        endpoint: &str,
        body: &str,
        sender: &crate::provider::StreamSender,
        mut parser: Box<dyn FnMut(Value) -> Result<Vec<StreamEvent>, ProviderError> + Send>,
    ) -> Result<(), ProviderError> {
        let mut req = ureq::post(endpoint)
            .timeout(self.timeout)
            .set("Content-Type", "application/json")
            .set("Accept", "text/event-stream");

        if let Some(ref key) = self.api_key
            && !key.is_empty() {
                if endpoint.contains("anthropic") {
                    req = req
                        .set("x-api-key", key)
                        .set("anthropic-version", "2023-06-01");
                }
                req = req.set("Authorization", &format!("Bearer {key}"));
            }
        for (k, v) in &self.headers {
            req = req.set(k, v);
        }

        let resp = req.send_string(body).map_err(|e| match e {
            ureq::Error::Status(code, resp) => {
                let text = resp.into_string().unwrap_or_default();
                ProviderError::Network(format!("HTTP {code}: {text}"))
            }
            ureq::Error::Transport(e) => ProviderError::Network(e.to_string()),
        })?;

        let reader = std::io::BufReader::new(resp.into_reader());
        use std::io::BufRead;
        let mut usage = None;
        let mut current_data = String::new();

        let mut process_data = |data: &str| -> Result<(), ProviderError> {
            let trimmed = data.trim();
            if trimmed.is_empty() || trimmed == "[DONE]" {
                return Ok(());
            }
            let value: Value = match serde_json::from_str(trimmed) {
                Ok(v) => v,
                Err(_) => return Ok(()),
            };

            usage = usage.or_else(|| value.get("usage").and_then(parse_usage));
            match parser(value) {
                Ok(events) => {
                    for event in events {
                        let event = match event {
                            StreamEvent::Finish { reason } => {
                                if let Some(u) = usage {
                                    let _ = sender.send(StreamEvent::Usage(u));
                                    let _ = sender.flush();
                                }
                                StreamEvent::Finish { reason }
                            }
                            other => other,
                        };
                        if sender.send(event).is_err() {
                            return Ok(());
                        }
                    }
                    let _ = sender.flush();
                    Ok(())
                }
                Err(e) => {
                    let _ = sender.send(StreamEvent::Error(e.to_string()));
                    let _ = sender.flush();
                    Err(e)
                }
            }
        };

        for line_result in reader.lines() {
            let line = match line_result {
                Ok(l) => l,
                Err(e) => {
                    let _ = sender.send(StreamEvent::Error(e.to_string()));
                    let _ = sender.flush();
                    return Err(ProviderError::Network(e.to_string()));
                }
            };
            let trimmed = line.trim();
            if trimmed.is_empty() {
                if !current_data.is_empty() {
                    let to_process = std::mem::take(&mut current_data);
                    if process_data(&to_process).is_err() {
                        return Ok(());
                    }
                }
                continue;
            }
            if trimmed.starts_with(':') {
                continue;
            }
            if trimmed == "data: [DONE]" || trimmed == "[DONE]" {
                break;
            }
            if let Some(rest) = trimmed.strip_prefix("data:") {
                let payload = rest.trim();
                if payload == "[DONE]" {
                    break;
                }
                if current_data.is_empty() && serde_json::from_str::<Value>(payload).is_ok() {
                    if process_data(payload).is_err() {
                        return Ok(());
                    }
                    continue;
                }
                if !current_data.is_empty() {
                    current_data.push('\n');
                }
                current_data.push_str(payload);
                if serde_json::from_str::<Value>(&current_data).is_ok() {
                    let to_process = std::mem::take(&mut current_data);
                    if process_data(&to_process).is_err() {
                        return Ok(());
                    }
                }
            } else if trimmed.starts_with("event:")
                || trimmed.starts_with("id:")
                || trimmed.starts_with("retry:")
            {
                continue;
            } else if trimmed.starts_with('{') || trimmed.starts_with('[') {
                if current_data.is_empty() && serde_json::from_str::<Value>(trimmed).is_ok() {
                    if process_data(trimmed).is_err() {
                        return Ok(());
                    }
                    continue;
                }
                if !current_data.is_empty() {
                    current_data.push('\n');
                }
                current_data.push_str(trimmed);
                if serde_json::from_str::<Value>(&current_data).is_ok() {
                    let to_process = std::mem::take(&mut current_data);
                    if process_data(&to_process).is_err() {
                        return Ok(());
                    }
                }
            }
        }
        if !current_data.is_empty() {
            let _ = process_data(&current_data);
        }
        let _ = sender.flush();
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct JsonProvider<T> {
    id: ProviderId,
    endpoint: String,
    capabilities: ProviderCapabilities,
    models: Vec<ModelInfo>,
    transport: T,
}

pub type OpenAiCompatible<T> = JsonProvider<T>;
pub type Anthropic<T> = JsonProvider<T>;
pub type Ollama<T> = JsonProvider<T>;

type StreamEventParser = Box<dyn FnMut(Value) -> Result<Vec<StreamEvent>, ProviderError> + Send>;

impl<T: Transport + Clone + 'static> JsonProvider<T> {
    pub fn new(id: impl Into<String>, endpoint: impl Into<String>, transport: T) -> Self {
        Self {
            id: ProviderId::new(id),
            endpoint: endpoint.into(),
            capabilities: ProviderCapabilities {
                streaming: true,
                tools: false,
            },
            models: Vec::new(),
            transport,
        }
    }

    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    pub fn transport(&self) -> &T {
        &self.transport
    }

    pub(crate) fn build_payload(
        &self,
        request: &StreamRequest,
    ) -> (String, StreamEventParser) {
        let messages: Vec<_> = if !request.messages.is_empty() {
            request
                .messages
                .iter()
                .map(|m| {
                    let mut obj = serde_json::json!({
                        "role": m.role,
                        "content": if m.role == "assistant" && m.tool_calls.is_some() && m.content.is_empty() {
                            serde_json::Value::Null
                        } else {
                            serde_json::Value::String(m.content.clone())
                        },
                    });
                    if let Some(name) = &m.name {
                        obj["name"] = serde_json::Value::String(name.clone());
                    }
                    if let Some(tid) = &m.tool_call_id {
                        obj["tool_call_id"] = serde_json::Value::String(tid.clone());
                    }
                    if let Some(tcalls) = &m.tool_calls {
                        obj["tool_calls"] = serde_json::Value::Array(tcalls.clone());
                    }
                    obj
                })
                .collect()
        } else {
            vec![serde_json::json!({
                "role": "user",
                "content": request.prompt,
            })]
        };

        let (body, parser): (serde_json::Value, StreamEventParser) = match self.id.as_str() {
            "anthropic" => {
                let mut system_prompt = None;
                let mut anthropic_messages = Vec::new();

                for m in &request.messages {
                    if m.role == "system" {
                        system_prompt = Some(m.content.clone());
                    } else if m.role == "tool" {
                        anthropic_messages.push(serde_json::json!({
                            "role": "user",
                            "content": [{
                                "type": "tool_result",
                                "tool_use_id": m.tool_call_id.clone().unwrap_or_default(),
                                "content": m.content,
                            }]
                        }));
                    } else if m.role == "assistant" && m.tool_calls.is_some() {
                        let mut contents = Vec::new();
                        if !m.content.is_empty() {
                            contents.push(serde_json::json!({
                                "type": "text",
                                "text": m.content,
                            }));
                        }
                        if let Some(tcalls) = &m.tool_calls {
                            for call in tcalls {
                                let id = call.get("id").and_then(Value::as_str).unwrap_or("");
                                let name = call
                                    .pointer("/function/name")
                                    .and_then(Value::as_str)
                                    .unwrap_or("");
                                let args_val: Value = call
                                    .pointer("/function/arguments")
                                    .and_then(|a| match a {
                                        Value::String(s) => serde_json::from_str(s).ok(),
                                        other => Some(other.clone()),
                                    })
                                    .unwrap_or_else(|| serde_json::json!({}));
                                contents.push(serde_json::json!({
                                    "type": "tool_use",
                                    "id": id,
                                    "name": name,
                                    "input": args_val,
                                }));
                            }
                        }
                        anthropic_messages.push(serde_json::json!({
                            "role": "assistant",
                            "content": contents,
                        }));
                    } else {
                        anthropic_messages.push(serde_json::json!({
                            "role": m.role,
                            "content": m.content,
                        }));
                    }
                }

                if anthropic_messages.is_empty() {
                    anthropic_messages.push(serde_json::json!({
                        "role": "user",
                        "content": request.prompt,
                    }));
                }

                let mut payload = serde_json::json!({
                    "model": request.model,
                    "messages": anthropic_messages,
                    "max_tokens": request.max_output_tokens,
                    "stream": true,
                });
                if let Some(sys) = system_prompt {
                    payload["system"] = serde_json::Value::String(sys);
                }
                if !request.tools.is_empty() {
                    let anthropic_tools: Vec<Value> = request
                        .tools
                        .iter()
                        .filter_map(|t| {
                            let func = t.get("function")?;
                            let name = func.get("name")?;
                            let desc = func
                                .get("description")
                                .cloned()
                                .unwrap_or_else(|| serde_json::json!(""));
                            let params = func
                                .get("parameters")
                                .cloned()
                                .unwrap_or_else(|| serde_json::json!({}));
                            Some(serde_json::json!({
                                "name": name,
                                "description": desc,
                                "input_schema": params,
                            }))
                        })
                        .collect();
                    if !anthropic_tools.is_empty() {
                        payload["tools"] = serde_json::Value::Array(anthropic_tools);
                    }
                }
                (payload, Box::new(anthropic_parser()))
            }
            "ollama" => {
                if self.endpoint.contains("/api/chat") {
                    let mut payload = serde_json::json!({
                        "model": request.model,
                        "messages": messages,
                        "stream": true,
                    });
                    if !request.tools.is_empty() {
                        payload["tools"] = serde_json::Value::Array(request.tools.clone());
                    }
                    (payload, Box::new(ollama_parser()))
                } else {
                    (
                        serde_json::json!({
                            "model": request.model,
                            "prompt": request.prompt,
                            "stream": true,
                        }),
                        Box::new(ollama_parser()),
                    )
                }
            }
            _ => {
                let mut payload = serde_json::json!({
                    "model": request.model,
                    "messages": messages,
                    "max_tokens": request.max_output_tokens,
                    "stream": true,
                });
                if !request.tools.is_empty() {
                    payload["tools"] = serde_json::Value::Array(request.tools.clone());
                }
                (payload, Box::new(openai_parser()))
            }
        };

        (body.to_string(), parser)
    }

    pub(crate) fn request_body(&self, request: &StreamRequest) -> Result<String, ProviderError> {
        let (body, _) = self.build_payload(request);
        self.transport.request(&self.endpoint, &body)
    }

    pub(crate) fn stream_body(
        &self,
        request: &StreamRequest,
        sender: &crate::provider::StreamSender,
    ) -> Result<(), ProviderError> {
        let (body, parser) = self.build_payload(request);
        self.transport
            .stream_request(&self.endpoint, &body, sender, parser)
    }

    pub fn with_models(mut self, models: Vec<ModelInfo>) -> Self {
        self.models = models;
        self
    }
    pub fn with_capabilities(mut self, capabilities: ProviderCapabilities) -> Self {
        self.capabilities = capabilities;
        self
    }
    pub fn parse_openai(body: &str) -> Result<StreamResponse, ProviderError> {
        let mut p = openai_parser();
        parse_lines(body, &mut p)
    }
    pub fn parse_anthropic(body: &str) -> Result<StreamResponse, ProviderError> {
        let mut p = anthropic_parser();
        parse_lines(body, &mut p)
    }
    pub fn parse_ollama(body: &str) -> Result<StreamResponse, ProviderError> {
        let mut p = ollama_parser();
        parse_lines(body, &mut p)
    }
}

impl<T: Transport + Clone + 'static> crate::provider::Provider for JsonProvider<T> {
    fn id(&self) -> &ProviderId {
        &self.id
    }
    fn capabilities(&self) -> ProviderCapabilities {
        self.capabilities
    }
    fn models(&self) -> Vec<ModelInfo> {
        self.models.clone()
    }
    fn send(&self, request: &StreamRequest) -> Result<StreamResponse, ProviderError> {
        let response = self.request_body(request)?;
        match self.id.as_str() {
            "anthropic" => Self::parse_anthropic(&response),
            "ollama" => Self::parse_ollama(&response),
            _ => Self::parse_openai(&response),
        }
    }
    fn stream(
        &self,
        request: &StreamRequest,
    ) -> Result<crate::provider::ProviderStream, ProviderError> {
        let (sender, stream) = crate::provider::ProviderStream::channel(256);
        let this = self.clone();
        let req = request.clone();
        std::thread::spawn(move || {
            let _ = this.stream_body(&req, &sender);
        });
        Ok(stream)
    }
}

fn parse_lines(
    body: &str,
    parser: &mut (dyn FnMut(Value) -> Result<Vec<StreamEvent>, ProviderError> + Send),
) -> Result<StreamResponse, ProviderError> {
    let mut events = Vec::new();
    let mut usage = None;

    let trimmed_body = body.trim();
    if (trimmed_body.starts_with('{') || trimmed_body.starts_with('['))
        && let Ok(value) = serde_json::from_str::<Value>(trimmed_body) {
            let raw_usage = value.get("usage").and_then(parse_usage);
            for event in parser(value)? {
                let event = match event {
                    StreamEvent::Finish { reason } => {
                        if let Some(value) = raw_usage {
                            events.push(StreamEvent::Usage(value));
                        }
                        StreamEvent::Finish { reason }
                    }
                    other => other,
                };
                events.push(event);
            }
            return Ok(StreamResponse { events });
        }

    let mut current_data = String::new();
    let mut parse_block = |data: &str,
                           events: &mut Vec<StreamEvent>,
                           usage: &mut Option<crate::provider::Usage>|
     -> Result<(), ProviderError> {
        let trimmed = data.trim();
        if trimmed.is_empty() || trimmed == "[DONE]" {
            return Ok(());
        }
        let value: Value = serde_json::from_str(trimmed)
            .map_err(|error| ProviderError::Protocol(error.to_string()))?;
        *usage = usage.or_else(|| value.get("usage").and_then(parse_usage));
        for event in parser(value)? {
            let event = match event {
                StreamEvent::Finish { reason } => {
                    if let Some(value) = *usage {
                        events.push(StreamEvent::Usage(value));
                    }
                    StreamEvent::Finish { reason }
                }
                other => other,
            };
            events.push(event);
        }
        Ok(())
    };

    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if !current_data.is_empty() {
                let data = std::mem::take(&mut current_data);
                parse_block(&data, &mut events, &mut usage)?;
            }
            continue;
        }
        if trimmed.starts_with(':') {
            continue;
        }
        if trimmed == "data: [DONE]" || trimmed == "[DONE]" {
            break;
        }
        if let Some(rest) = trimmed.strip_prefix("data:") {
            let payload = rest.trim();
            if payload == "[DONE]" {
                break;
            }
            if current_data.is_empty() && serde_json::from_str::<Value>(payload).is_ok() {
                parse_block(payload, &mut events, &mut usage)?;
                continue;
            }
            if !current_data.is_empty() {
                current_data.push('\n');
            }
            current_data.push_str(payload);
            if serde_json::from_str::<Value>(&current_data).is_ok() {
                let data = std::mem::take(&mut current_data);
                parse_block(&data, &mut events, &mut usage)?;
            }
        } else if trimmed.starts_with("event:")
            || trimmed.starts_with("id:")
            || trimmed.starts_with("retry:")
        {
            continue;
        } else if trimmed.starts_with('{') || trimmed.starts_with('[') {
            if current_data.is_empty() && serde_json::from_str::<Value>(trimmed).is_ok() {
                parse_block(trimmed, &mut events, &mut usage)?;
                continue;
            }
            if !current_data.is_empty() {
                current_data.push('\n');
            }
            current_data.push_str(trimmed);
            if serde_json::from_str::<Value>(&current_data).is_ok() {
                let data = std::mem::take(&mut current_data);
                parse_block(&data, &mut events, &mut usage)?;
            }
        }
    }

    if !current_data.is_empty() {
        parse_block(&current_data, &mut events, &mut usage)?;
    }

    Ok(StreamResponse { events })
}

fn parse_usage(value: &Value) -> Option<crate::provider::Usage> {
    Some(crate::provider::Usage {
        input_tokens: value
            .get("prompt_tokens")
            .or_else(|| value.get("input_tokens"))
            .or_else(|| value.get("prompt_eval_count"))
            .and_then(Value::as_u64)?,
        output_tokens: value
            .get("completion_tokens")
            .or_else(|| value.get("output_tokens"))
            .or_else(|| value.get("eval_count"))
            .and_then(Value::as_u64)?,
    })
}

pub(crate) fn openai_parser() -> impl FnMut(Value) -> Result<Vec<StreamEvent>, ProviderError> + Send
{
    let mut active_tools: std::collections::HashMap<usize, String> =
        std::collections::HashMap::new();
    move |value: Value| -> Result<Vec<StreamEvent>, ProviderError> {
        let mut events = Vec::new();
        if let Some(error) = value.get("error").and_then(Value::as_str) {
            return Err(ProviderError::Protocol(error.into()));
        }
        if let Some(error_obj) = value.get("error").and_then(Value::as_object)
            && let Some(msg) = error_obj.get("message").and_then(Value::as_str) {
                return Err(ProviderError::Protocol(msg.into()));
            }

        // Content
        if let Some(s) = value
            .pointer("/choices/0/delta/content")
            .or_else(|| value.pointer("/choices/0/message/content"))
            .and_then(Value::as_str)
            && !s.is_empty() {
                events.push(StreamEvent::TextDelta(s.into()));
            }

        // Reasoning
        if let Some(s) = value
            .pointer("/choices/0/delta/reasoning_content")
            .or_else(|| value.pointer("/choices/0/delta/reasoning"))
            .and_then(Value::as_str)
            && !s.is_empty() {
                events.push(StreamEvent::ReasoningDelta(s.into()));
            }

        // Streaming tool_calls delta
        if let Some(tool_calls) = value
            .pointer("/choices/0/delta/tool_calls")
            .and_then(Value::as_array)
        {
            for call in tool_calls {
                let index = call.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                if let Some(id) = call.get("id").and_then(Value::as_str) {
                    let id_str = id.to_string();
                    active_tools.insert(index, id_str.clone());
                    let name = call
                        .pointer("/function/name")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    events.push(StreamEvent::ToolCallStart { id: id_str, name });
                }
                let call_id = active_tools.get(&index).cloned();
                if let Some(id) = call_id
                    && let Some(args) = call.pointer("/function/arguments").and_then(Value::as_str)
                        && !args.is_empty() {
                            events.push(StreamEvent::ToolCallDelta {
                                id,
                                arguments: args.to_string(),
                            });
                        }
            }
        }

        // Non-streaming / message tool_calls
        if let Some(tool_calls) = value
            .pointer("/choices/0/message/tool_calls")
            .and_then(Value::as_array)
        {
            for (idx, call) in tool_calls.iter().enumerate() {
                let id = call
                    .get("id")
                    .and_then(Value::as_str)
                    .map(String::from)
                    .unwrap_or_else(|| format!("call_{idx}"));
                let name = call
                    .pointer("/function/name")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let args = call
                    .pointer("/function/arguments")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();

                events.push(StreamEvent::ToolCallStart {
                    id: id.clone(),
                    name,
                });
                if !args.is_empty() {
                    events.push(StreamEvent::ToolCallDelta {
                        id: id.clone(),
                        arguments: args,
                    });
                }
                events.push(StreamEvent::ToolCallEnd { id });
            }
        }

        // Finish reason
        if let Some(finish_reason) = value
            .pointer("/choices/0/finish_reason")
            .and_then(Value::as_str)
        {
            for (_, id) in active_tools.drain() {
                events.push(StreamEvent::ToolCallEnd { id });
            }
            events.push(finish_event(finish_reason));
        }

        Ok(events)
    }
}

pub(crate) fn anthropic_parser()
-> impl FnMut(Value) -> Result<Vec<StreamEvent>, ProviderError> + Send {
    let mut active_tools: std::collections::HashMap<usize, (String, String)> =
        std::collections::HashMap::new();
    move |value: Value| -> Result<Vec<StreamEvent>, ProviderError> {
        let mut events = Vec::new();
        if value.get("type").and_then(Value::as_str) == Some("error") {
            let msg = value
                .pointer("/error/message")
                .and_then(Value::as_str)
                .unwrap_or("anthropic error");
            return Err(ProviderError::Protocol(msg.into()));
        }

        // Streaming text
        if let Some(s) = value
            .pointer("/delta/text")
            .or_else(|| value.pointer("/content/0/text"))
            .and_then(Value::as_str)
            && !s.is_empty() {
                events.push(StreamEvent::TextDelta(s.into()));
            }

        // Streaming content_block_start for tool_use
        if value.get("type").and_then(Value::as_str) == Some("content_block_start") {
            let index = value.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
            if let Some(cb) = value.get("content_block").and_then(Value::as_object)
                && cb.get("type").and_then(Value::as_str) == Some("tool_use") {
                    let id = cb
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    let name = cb
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    active_tools.insert(index, (id.clone(), name.clone()));
                    events.push(StreamEvent::ToolCallStart { id, name });
                }
        }

        // Streaming content_block_delta for tool_use (input_json_delta)
        if value.get("type").and_then(Value::as_str) == Some("content_block_delta") {
            let index = value.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
            if let Some(partial) = value.pointer("/delta/partial_json").and_then(Value::as_str)
                && !partial.is_empty()
                    && let Some((id, _)) = active_tools.get(&index) {
                        events.push(StreamEvent::ToolCallDelta {
                            id: id.clone(),
                            arguments: partial.to_string(),
                        });
                    }
        }

        // Streaming content_block_stop
        if value.get("type").and_then(Value::as_str) == Some("content_block_stop") {
            let index = value.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
            if let Some((id, _)) = active_tools.remove(&index) {
                events.push(StreamEvent::ToolCallEnd { id });
            }
        }

        // Non-streaming / message content array with tool_use
        if let Some(content_array) = value.get("content").and_then(Value::as_array) {
            for (idx, block) in content_array.iter().enumerate() {
                if block.get("type").and_then(Value::as_str) == Some("tool_use") {
                    let id = block
                        .get("id")
                        .and_then(Value::as_str)
                        .map(String::from)
                        .unwrap_or_else(|| format!("call_{idx}"));
                    let name = block
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    let args = block
                        .get("input")
                        .map(|v| v.to_string())
                        .unwrap_or_default();
                    events.push(StreamEvent::ToolCallStart {
                        id: id.clone(),
                        name,
                    });
                    if !args.is_empty() {
                        events.push(StreamEvent::ToolCallDelta {
                            id: id.clone(),
                            arguments: args,
                        });
                    }
                    events.push(StreamEvent::ToolCallEnd { id });
                }
            }
        }

        // Stop reason (streaming in message_delta, or non-streaming in top-level stop_reason)
        let stop_reason = value
            .pointer("/delta/stop_reason")
            .or_else(|| value.get("stop_reason"))
            .and_then(Value::as_str);
        if let Some(stop) = stop_reason {
            for (_, (id, _)) in active_tools.drain() {
                events.push(StreamEvent::ToolCallEnd { id });
            }
            events.push(finish_event(stop));
        }

        Ok(events)
    }
}

pub(crate) fn ollama_parser() -> impl FnMut(Value) -> Result<Vec<StreamEvent>, ProviderError> + Send
{
    move |value: Value| -> Result<Vec<StreamEvent>, ProviderError> {
        let mut events = Vec::new();
        if let Some(error) = value.get("error").and_then(Value::as_str) {
            return Err(ProviderError::Protocol(error.into()));
        }
        if let Some(s) = value
            .pointer("/message/content")
            .or_else(|| value.get("response"))
            .and_then(Value::as_str)
            && !s.is_empty() {
                events.push(StreamEvent::TextDelta(s.into()));
            }
        let mut has_tool_calls = false;
        if let Some(tool_calls) = value
            .pointer("/message/tool_calls")
            .and_then(Value::as_array)
        {
            for (idx, call) in tool_calls.iter().enumerate() {
                has_tool_calls = true;
                let id = call
                    .get("id")
                    .and_then(Value::as_str)
                    .map(String::from)
                    .unwrap_or_else(|| format!("ollama_call_{idx}"));
                let name = call
                    .pointer("/function/name")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let args = match call.pointer("/function/arguments") {
                    Some(Value::String(s)) => s.clone(),
                    Some(val) => val.to_string(),
                    None => String::new(),
                };

                events.push(StreamEvent::ToolCallStart {
                    id: id.clone(),
                    name,
                });
                if !args.is_empty() {
                    events.push(StreamEvent::ToolCallDelta {
                        id: id.clone(),
                        arguments: args,
                    });
                }
                events.push(StreamEvent::ToolCallEnd { id });
            }
        }
        if let Some(done) = value.get("done").and_then(Value::as_bool)
            && done {
                let reason = if has_tool_calls { "tool_calls" } else { "stop" };
                events.push(finish_event(reason));
            }
        Ok(events)
    }
}

fn finish_event(reason: &str) -> StreamEvent {
    StreamEvent::Finish {
        reason: match reason {
            "length" | "max_tokens" => FinishReason::Length,
            "tool_calls" | "tool_use" => FinishReason::ToolCall,
            _ => FinishReason::Stop,
        },
    }
}

/// Dynamic router provider that constructs appropriate concrete adapters
/// from application configuration based on the requested provider and model.
#[derive(Clone, Debug)]
pub struct ConfiguredRouter {
    config: std::sync::Arc<crate::config::Config>,
    default_provider_id: ProviderId,
}

impl ConfiguredRouter {
    pub fn new(config: crate::config::Config) -> Self {
        let (p, _) = config.initial_provider_and_model();
        let default_id = if !p.is_empty() {
            p
        } else if let Some(first) = config.providers.keys().next() {
            first.clone()
        } else {
            "openai".to_string()
        };
        Self {
            config: std::sync::Arc::new(config),
            default_provider_id: ProviderId::new(default_id),
        }
    }

    fn resolve_provider(
        &self,
        request: &StreamRequest,
    ) -> (String, String, Option<String>, StreamRequest) {
        let mut req = request.clone();
        let mut p_name = request
            .provider
            .clone()
            .unwrap_or_else(|| self.default_provider_id.as_str().to_string());

        let split = req
            .model
            .split_once('/')
            .map(|(p, m)| (p.to_string(), m.to_string()));

        if let Some((prefix, stripped)) = split
            && self.config.providers.contains_key(&prefix) {
                p_name = prefix;
                req.model = stripped;
            }

        if let Some(stripped) = req.model.strip_prefix(&format!("{p_name}/")) {
            req.model = stripped.to_string();
        }

        let cfg = self.config.providers.get(&p_name);
        let base_url = cfg
            .and_then(|c| c.base_url.as_deref())
            .or_else(|| self.config.resolved_endpoint(Some(&p_name)))
            .unwrap_or("https://api.openai.com/v1");

        let api_key = cfg
            .and_then(|c| c.resolved_api_key())
            .or_else(|| self.config.api_key.as_ref().and_then(|s| s.resolve().ok()))
            .or_else(|| match p_name.as_str() {
                "anthropic" => std::env::var("ANTHROPIC_API_KEY").ok(),
                "openai" => std::env::var("OPENAI_API_KEY").ok(),
                "openrouter" => std::env::var("OPENROUTER_API_KEY").ok(),
                _ => None,
            });

        let npm = cfg.and_then(|c| c.npm.as_deref()).unwrap_or("");
        let is_anthropic = npm.contains("anthropic") || p_name == "anthropic";
        let is_ollama = npm.contains("ollama") || p_name == "ollama";

        let endpoint = if is_anthropic {
            let base = base_url.trim_end_matches('/');
            if base.ends_with("/v1") {
                format!("{base}/messages")
            } else {
                format!("{base}/v1/messages")
            }
        } else if is_ollama {
            let base = base_url.trim_end_matches('/');
            if base.contains("/api/chat") {
                base.to_string()
            } else {
                format!("{base}/api/chat")
            }
        } else {
            let base = base_url.trim_end_matches('/');
            if base.ends_with("/chat/completions") {
                base.to_string()
            } else if base.ends_with("/v1") || base.contains("/v1") {
                format!("{base}/chat/completions")
            } else {
                format!("{base}/v1/chat/completions")
            }
        };

        (p_name, endpoint, api_key, req)
    }

    fn create_provider(
        &self,
        request: &StreamRequest,
    ) -> (JsonProvider<HttpTransport>, StreamRequest) {
        let (p_name, endpoint, api_key, req) = self.resolve_provider(request);
        let mut transport = HttpTransport::new();
        if let Some(key) = api_key {
            transport = transport.with_api_key(key);
        }
        let provider = JsonProvider::new(p_name, endpoint, transport);
        (provider, req)
    }
}

impl Provider for ConfiguredRouter {
    fn id(&self) -> &ProviderId {
        &self.default_provider_id
    }
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            streaming: true,
            tools: true,
        }
    }
    fn models(&self) -> Vec<ModelInfo> {
        let mut list = Vec::new();
        for (provider_name, cfg) in &self.config.providers {
            for m in cfg.to_model_infos() {
                list.push(ModelInfo {
                    id: format!("{provider_name}/{}", m.id),
                    context_window: m.context_window,
                });
                list.push(m);
            }
        }
        list
    }
    fn send(&self, request: &StreamRequest) -> Result<StreamResponse, ProviderError> {
        let (provider, req) = self.create_provider(request);
        provider.send(&req)
    }
    fn stream(
        &self,
        request: &StreamRequest,
    ) -> Result<crate::provider::ProviderStream, ProviderError> {
        let (provider, req) = self.create_provider(request);
        provider.stream(&req)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_openai_sse_text_usage_and_finish() {
        let body = "data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\ndata: {\"usage\":{\"prompt_tokens\":2,\"completion_tokens\":3},\"choices\":[{\"finish_reason\":\"stop\"}]}\ndata: [DONE]";
        let response = JsonProvider::<MockTransport>::parse_openai(body).unwrap();
        assert_eq!(response.text(), "hi");
        assert_eq!(response.usage().unwrap().output_tokens, 3);
    }

    #[test]
    fn parses_anthropic_and_ollama_text() {
        assert_eq!(
            JsonProvider::<MockTransport>::parse_anthropic("data: {\"delta\":{\"text\":\"a\"}}")
                .unwrap()
                .text(),
            "a"
        );
        assert_eq!(
            JsonProvider::<MockTransport>::parse_ollama("{\"response\":\"b\"}")
                .unwrap()
                .text(),
            "b"
        );
    }

    #[test]
    fn build_payload_serializes_assistant_tool_calls_with_null_content() {
        let provider = JsonProvider::new(
            "openai",
            "https://api.openai.com/v1/chat/completions",
            MockTransport("".into()),
        );
        let request = StreamRequest::new("gpt-4o", "", 100).with_messages(vec![
            crate::provider::ChatMessage {
                role: "assistant".into(),
                content: "".into(),
                tool_call_id: None,
                tool_calls: Some(vec![serde_json::json!({
                    "id": "call_123",
                    "type": "function",
                    "function": { "name": "read_file", "arguments": "{}" }
                })]),
                name: None,
            },
        ]);
        let (body, _) = provider.build_payload(&request);
        let parsed: serde_json::Value = serde_json::from_str(&body).unwrap();
        let msgs = parsed["messages"].as_array().unwrap();
        assert_eq!(msgs[0]["content"], serde_json::Value::Null);
    }

    #[test]
    fn build_payload_serializes_tool_message_with_name() {
        let provider = JsonProvider::new(
            "openai",
            "https://api.openai.com/v1/chat/completions",
            MockTransport("".into()),
        );
        let request = StreamRequest::new("gpt-4o", "", 100).with_messages(vec![
            crate::provider::ChatMessage {
                role: "tool".into(),
                content: "file content".into(),
                tool_call_id: Some("call_123".into()),
                tool_calls: None,
                name: Some("read_file".into()),
            },
        ]);
        let (body, _) = provider.build_payload(&request);
        let parsed: serde_json::Value = serde_json::from_str(&body).unwrap();
        let msgs = parsed["messages"].as_array().unwrap();
        assert_eq!(msgs[0]["role"], "tool");
        assert_eq!(msgs[0]["name"], "read_file");
        assert_eq!(msgs[0]["tool_call_id"], "call_123");
        assert_eq!(msgs[0]["content"], "file content");
    }
    #[test]
    fn parses_sse_with_comments_and_event_lines() {
        let body = ": keepalive ping\nevent: message\ndata: {\"choices\":[{\"delta\":{\"content\":\"hello\"}}]}\n: another comment\nevent: done\ndata: [DONE]\n";
        let response = JsonProvider::<MockTransport>::parse_openai(body).unwrap();
        assert_eq!(response.text(), "hello");
    }

    #[test]
    fn parses_sse_multiline_data() {
        let body =
            "data: {\"choices\":\ndata: [{\"delta\":{\"content\":\"multi\"}}]}\ndata: [DONE]";
        let response = JsonProvider::<MockTransport>::parse_openai(body).unwrap();
        assert_eq!(response.text(), "multi");
    }

    #[test]
    fn parses_anthropic_streaming_tool_use() {
        let body = r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_01","name":"read_file","input":{}}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"path\":\"foo.rs\"}"}}

event: content_block_stop
data: {"type":"content_block_stop","index":0}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"tool_use"}}
"#;
        let response = JsonProvider::<MockTransport>::parse_anthropic(body).unwrap();
        assert_eq!(response.events.len(), 4);
        assert!(
            matches!(&response.events[0], StreamEvent::ToolCallStart { id, name } if id == "toolu_01" && name == "read_file")
        );
        assert!(
            matches!(&response.events[1], StreamEvent::ToolCallDelta { id, arguments } if id == "toolu_01" && arguments.contains("foo.rs"))
        );
        assert!(matches!(&response.events[2], StreamEvent::ToolCallEnd { id } if id == "toolu_01"));
        assert!(
            matches!(&response.events[3], StreamEvent::Finish { reason } if *reason == crate::provider::FinishReason::ToolCall)
        );
    }

    #[test]
    fn parses_ollama_tool_calls() {
        let body = r#"{"model":"llama3","message":{"role":"assistant","content":"","tool_calls":[{"id":"call_1","function":{"name":"read_file","arguments":{"path":"src/main.rs"}}}]},"done":true}"#;
        let response = JsonProvider::<MockTransport>::parse_ollama(body).unwrap();
        assert!(
            matches!(&response.events[0], StreamEvent::ToolCallStart { id, name } if id == "call_1" && name == "read_file")
        );
        assert!(
            matches!(&response.events[1], StreamEvent::ToolCallDelta { id, arguments } if id == "call_1" && arguments.contains("src/main.rs"))
        );
        assert!(matches!(&response.events[2], StreamEvent::ToolCallEnd { id } if id == "call_1"));
        assert!(
            matches!(&response.events[3], StreamEvent::Finish { reason } if *reason == crate::provider::FinishReason::ToolCall)
        );
    }

    #[test]
    fn build_payload_anthropic_extracts_system_and_tools() {
        let provider = JsonProvider::new(
            "anthropic",
            "https://api.anthropic.com/v1/messages",
            MockTransport("".into()),
        );
        let request = StreamRequest::new("claude-3-opus", "", 100)
            .with_messages(vec![
                crate::provider::ChatMessage {
                    role: "system".into(),
                    content: "system instructions".into(),
                    tool_call_id: None,
                    tool_calls: None,
                    name: None,
                },
                crate::provider::ChatMessage {
                    role: "user".into(),
                    content: "hello".into(),
                    tool_call_id: None,
                    tool_calls: None,
                    name: None,
                },
            ])
            .with_tools(vec![serde_json::json!({
                "type": "function",
                "function": {
                    "name": "edit_file",
                    "description": "Edits file",
                    "parameters": { "type": "object" }
                }
            })]);
        let (body, _) = provider.build_payload(&request);
        let parsed: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(parsed["system"], "system instructions");
        let msgs = parsed["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["role"], "user");
        let tools = parsed["tools"].as_array().unwrap();
        assert_eq!(tools[0]["name"], "edit_file");
        assert_eq!(tools[0]["description"], "Edits file");
        assert!(tools[0]["input_schema"].is_object());
    }
}
