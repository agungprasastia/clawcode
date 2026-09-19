use clawcode::adapters::{
    MockTransport, anthropic::Anthropic, ollama::Ollama, openai_compatible::OpenAiCompatible,
};
use clawcode::provider::{Provider, StreamRequest};

fn request() -> StreamRequest {
    StreamRequest::new("mock", "hello", 32)
}

#[test]
fn openai_compatible_normalizes_sse_fixture() {
    let provider = OpenAiCompatible::new(
        "openai",
        "https://example.test/v1/chat/completions",
        MockTransport("data: {\"choices\":[{\"delta\":{\"content\":\"hello\"}}]}\ndata: {\"choices\":[{\"finish_reason\":\"stop\"}]}\ndata: [DONE]".into()),
    );
    assert_eq!(provider.send(&request()).unwrap().text(), "hello");
}

#[test]
fn native_adapters_use_their_protocols() {
    let anthropic = Anthropic::new(
        "anthropic",
        "https://example.test/messages",
        MockTransport("data: {\"delta\":{\"text\":\"hi\"}}".into()),
    );
    let ollama = Ollama::new(
        "ollama",
        "http://localhost:11434/api/generate",
        MockTransport("{\"response\":\"local\"}".into()),
    );
    assert_eq!(anthropic.send(&request()).unwrap().text(), "hi");
    assert_eq!(ollama.send(&request()).unwrap().text(), "local");
}

#[test]
fn provider_errors_are_not_silenced() {
    let provider = OpenAiCompatible::new(
        "openai",
        "https://example.test",
        MockTransport("data: {\"error\":\"quota\"}".into()),
    );
    assert!(provider.send(&request()).is_err());
}

#[test]
fn openai_compatible_streams_sse_deltas() {
    let provider = OpenAiCompatible::new(
        "openai",
        "https://example.test/v1/chat/completions",
        MockTransport("data: {\"choices\":[{\"delta\":{\"content\":\"streamed \"}}]}\ndata: {\"choices\":[{\"delta\":{\"content\":\"response\"}}]}\ndata: [DONE]".into()),
    );
    let mut stream = provider.stream(&request()).expect("stream start");
    let mut tokens = String::new();
    for event in &mut stream {
        if let clawcode::provider::StreamEvent::TextDelta(delta) = event {
            tokens.push_str(&delta);
        }
    }
    assert_eq!(tokens, "streamed response");
}

#[test]
fn configured_router_resolves_and_discovers_models() {
    let mut config = clawcode::config::Config::default();
    let mut custom = clawcode::config::CustomProviderConfig {
        name: Some("9router".into()),
        base_url: Some("http://127.0.0.1:20128/v1".into()),
        api_key: Some("test-key".into()),
        npm: Some("@ai-sdk/openai-compatible".into()),
        models: std::collections::BTreeMap::new(),
    };
    custom.models.insert(
        "ag/gemini-3.8-flash-high".into(),
        clawcode::config::CustomModelConfig {
            name: Some("Gemini 3.8 Flash High".into()),
            ..Default::default()
        },
    );
    config.providers.insert("9router".into(), custom);
    let router = clawcode::adapters::ConfiguredRouter::new(config);
    let models = router.models();
    assert!(
        models
            .iter()
            .any(|m| m.id == "9router/ag/gemini-3.8-flash-high")
    );
}

#[test]
fn openai_streaming_parallel_tool_calls() {
    let sse_data = "\
data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_0\",\"type\":\"function\",\"function\":{\"name\":\"func_0\",\"arguments\":\"\"}},{\"index\":1,\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"func_1\",\"arguments\":\"\"}}]},\"finish_reason\":null}]}\n\
data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":1,\"function\":{\"arguments\":\"{\\\"b\\\":\"}}]},\"finish_reason\":null}]}\n\
data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"a\\\": 1}\"}}]},\"finish_reason\":null}]}\n\
data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":1,\"function\":{\"arguments\":\"2}\"}}]},\"finish_reason\":null}]}\n\
data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\
data: [DONE]\n";

    let provider = OpenAiCompatible::new(
        "openai",
        "https://example.test/v1/chat/completions",
        MockTransport(sse_data.into()),
    );
    let mut stream = provider.stream(&request()).expect("stream start");
    let mut events = Vec::new();
    for event in &mut stream {
        events.push(event);
    }

    assert!(events.iter().any(|e| matches!(e, clawcode::provider::StreamEvent::ToolCallStart { id, name } if id == "call_0" && name == "func_0")));
    assert!(events.iter().any(|e| matches!(e, clawcode::provider::StreamEvent::ToolCallStart { id, name } if id == "call_1" && name == "func_1")));

    let starts_count = events
        .iter()
        .filter(|e| matches!(e, clawcode::provider::StreamEvent::ToolCallStart { .. }))
        .count();
    assert_eq!(starts_count, 2);

    let ends: Vec<&str> = events
        .iter()
        .filter_map(|e| match e {
            clawcode::provider::StreamEvent::ToolCallEnd { id } => Some(id.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(ends, vec!["call_0", "call_1"]);
}

#[test]
fn openai_streaming_unclosed_tool_call_ends_on_done() {
    let sse_data = "\
data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_unclosed\",\"function\":{\"name\":\"do_something\",\"arguments\":\"{\\\"x\\\":1}\"}}]},\"finish_reason\":null}]}\n\
data: [DONE]\n";

    let provider = OpenAiCompatible::new(
        "openai",
        "https://example.test/v1/chat/completions",
        MockTransport(sse_data.into()),
    );
    let mut stream = provider.stream(&request()).expect("stream start");
    let mut events = Vec::new();
    for event in &mut stream {
        events.push(event);
    }

    assert!(events.iter().any(|e| matches!(e, clawcode::provider::StreamEvent::ToolCallStart { id, .. } if id == "call_unclosed")));
    assert!(events.iter().any(|e| matches!(e, clawcode::provider::StreamEvent::ToolCallEnd { id } if id == "call_unclosed")));
}

#[test]
fn openai_streaming_usage_in_final_chunk() {
    let sse_data = "\
data: {\"choices\":[{\"delta\":{\"content\":\"done.\"},\"finish_reason\":\"stop\"}]}\n\
data: {\"choices\":[],\"usage\":{\"prompt_tokens\":12,\"completion_tokens\":34}}\n\
data: [DONE]\n";

    let provider = OpenAiCompatible::new(
        "openai",
        "https://example.test/v1/chat/completions",
        MockTransport(sse_data.into()),
    );
    let mut stream = provider.stream(&request()).expect("stream start");
    let mut events = Vec::new();
    for event in &mut stream {
        events.push(event);
    }

    let usage = events.iter().find_map(|e| match e {
        clawcode::provider::StreamEvent::Usage(u) => Some(*u),
        _ => None,
    });
    assert_eq!(
        usage,
        Some(clawcode::provider::Usage {
            input_tokens: 12,
            output_tokens: 34,
        })
    );
}

#[test]
fn sse_reader_preserves_whitespace_and_indentation() {
    let code_snippet = "    def foo():\n        return 42  \n";
    let json_chunk = serde_json::json!({
        "choices": [{
            "delta": { "content": code_snippet },
            "finish_reason": null
        }]
    });
    let sse_data = format!("data: {}\ndata: [DONE]\n", json_chunk);

    let provider = OpenAiCompatible::new(
        "openai",
        "https://example.test/v1/chat/completions",
        MockTransport(sse_data),
    );
    let mut stream = provider.stream(&request()).expect("stream start");
    let mut tokens = String::new();
    for event in &mut stream {
        if let clawcode::provider::StreamEvent::TextDelta(delta) = event {
            tokens.push_str(&delta);
        }
    }
    assert_eq!(tokens, code_snippet);
}

#[derive(Debug, Clone)]
struct CaptureTransport {
    response: String,
    last_body: std::sync::Arc<std::sync::Mutex<Option<String>>>,
}

impl CaptureTransport {
    fn new(response: impl Into<String>) -> Self {
        Self {
            response: response.into(),
            last_body: std::sync::Arc::new(std::sync::Mutex::new(None)),
        }
    }
    fn last_body(&self) -> Option<String> {
        match self.last_body.lock() {
            Ok(guard) => guard.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }
}

impl clawcode::adapters::Transport for CaptureTransport {
    fn request(
        &self,
        _endpoint: &str,
        body: &str,
    ) -> Result<String, clawcode::provider::ProviderError> {
        match self.last_body.lock() {
            Ok(mut guard) => *guard = Some(body.to_string()),
            Err(poisoned) => *poisoned.into_inner() = Some(body.to_string()),
        }
        Ok(self.response.clone())
    }
}

#[test]
fn anthropic_alternates_roles_and_merges_tool_result_into_user_string() {
    let transport = CaptureTransport::new("data: {\"delta\":{\"text\":\"hi\"}}\n\n");
    let provider = Anthropic::new(
        "anthropic",
        "https://api.anthropic.com/v1/messages",
        transport.clone(),
    );
    let request = StreamRequest::new("claude-3-5-sonnet", "", 100).with_messages(vec![
        clawcode::provider::ChatMessage {
            role: "user".into(),
            content: "first question".into(),
            tool_call_id: None,
            tool_calls: None,
            name: None,
        },
        clawcode::provider::ChatMessage {
            role: "user".into(),
            content: "second question".into(),
            tool_call_id: None,
            tool_calls: None,
            name: None,
        },
        clawcode::provider::ChatMessage {
            role: "tool".into(),
            content: "search result".into(),
            tool_call_id: Some("call_abc".into()),
            tool_calls: None,
            name: Some("search".into()),
        },
        clawcode::provider::ChatMessage {
            role: "assistant".into(),
            content: "step 1".into(),
            tool_call_id: None,
            tool_calls: None,
            name: None,
        },
        clawcode::provider::ChatMessage {
            role: "assistant".into(),
            content: "step 2".into(),
            tool_call_id: None,
            tool_calls: None,
            name: None,
        },
    ]);

    let _ = provider.send(&request);
    let body = transport.last_body().expect("captured request body");
    let parsed: serde_json::Value = serde_json::from_str(&body).unwrap();
    let msgs = parsed["messages"].as_array().unwrap();

    // Strict alternation: [user, assistant]
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[0]["role"], "user");
    assert_eq!(msgs[1]["role"], "assistant");

    // User string merged and converted into array with tool_result
    let user_content = msgs[0]["content"].as_array().unwrap();
    assert_eq!(user_content.len(), 2);
    assert_eq!(user_content[0]["type"], "text");
    assert_eq!(user_content[0]["text"], "first question\n\nsecond question");
    assert_eq!(user_content[1]["type"], "tool_result");
    assert_eq!(user_content[1]["tool_use_id"], "call_abc");
    assert_eq!(user_content[1]["content"], "search result");

    // Consecutive assistant messages merged
    assert_eq!(msgs[1]["content"], "step 1\n\nstep 2");
}

#[test]
fn anthropic_fallback_id_when_tool_call_id_empty() {
    let transport = CaptureTransport::new("data: {\"delta\":{\"text\":\"hi\"}}\n\n");
    let provider = Anthropic::new(
        "anthropic",
        "https://api.anthropic.com/v1/messages",
        transport.clone(),
    );
    let request = StreamRequest::new("claude-3-5-sonnet", "", 100).with_messages(vec![
        clawcode::provider::ChatMessage {
            role: "tool".into(),
            content: "output".into(),
            tool_call_id: Some("".into()),
            tool_calls: None,
            name: None,
        },
    ]);

    let _ = provider.send(&request);
    let body = transport.last_body().expect("captured request body");
    let parsed: serde_json::Value = serde_json::from_str(&body).unwrap();
    let msgs = parsed["messages"].as_array().unwrap();
    let content = msgs[0]["content"].as_array().unwrap();
    let tool_use_id = content[0]["tool_use_id"].as_str().unwrap();
    assert!(!tool_use_id.is_empty());
}

#[derive(Debug, Clone)]
struct FailingTransport;

impl clawcode::adapters::Transport for FailingTransport {
    fn request(
        &self,
        _endpoint: &str,
        _body: &str,
    ) -> Result<String, clawcode::provider::ProviderError> {
        Err(clawcode::provider::ProviderError::Network(
            "simulated network failure".into(),
        ))
    }
}

#[test]
fn streaming_propagates_transport_error() {
    let provider = OpenAiCompatible::new(
        "openai",
        "https://example.test/v1/chat/completions",
        FailingTransport,
    );
    let mut stream = provider.stream(&request()).expect("stream start");
    let first = stream.next();
    assert!(matches!(
        first,
        Some(clawcode::provider::StreamEvent::Error(msg)) if msg.contains("simulated network failure")
    ));
}

#[test]
fn configured_router_trims_env_api_keys() {
    let config = clawcode::config::Config::default();
    let router = clawcode::adapters::ConfiguredRouter::new(config);

    // Anthropic trimmed
    unsafe { std::env::set_var("ANTHROPIC_API_KEY", "  sk-ant-test-123  \n") };
    let req = StreamRequest::new("claude-3-5-sonnet", "", 10).with_provider("anthropic");
    let (_, _, key, _) = router.resolve_provider(&req);
    assert_eq!(key.as_deref(), Some("sk-ant-test-123"));

    // OpenAI trimmed
    unsafe { std::env::set_var("OPENAI_API_KEY", "\t sk-openai-test-456 \t") };
    let req = StreamRequest::new("gpt-4o", "", 10).with_provider("openai");
    let (_, _, key, _) = router.resolve_provider(&req);
    assert_eq!(key.as_deref(), Some("sk-openai-test-456"));

    // OpenRouter trimmed
    unsafe { std::env::set_var("OPENROUTER_API_KEY", " sk-or-test-789 ") };
    let req = StreamRequest::new("meta/llama-3", "", 10).with_provider("openrouter");
    let (_, _, key, _) = router.resolve_provider(&req);
    assert_eq!(key.as_deref(), Some("sk-or-test-789"));

    // Gemini / Google trimmed
    unsafe {
        std::env::remove_var("GEMINI_API_KEY");
        std::env::set_var("GOOGLE_API_KEY", " google-key-trim ");
    };
    let req = StreamRequest::new("gemini-1.5-pro", "", 10).with_provider("google");
    let (_, _, key, _) = router.resolve_provider(&req);
    assert_eq!(key.as_deref(), Some("google-key-trim"));

    // Whitespace-only considered empty/None
    unsafe { std::env::set_var("ANTHROPIC_API_KEY", "   \n\t  ") };
    let req = StreamRequest::new("claude-3-5-sonnet", "", 10).with_provider("anthropic");
    let (_, _, key, _) = router.resolve_provider(&req);
    assert_eq!(key, None);
}
