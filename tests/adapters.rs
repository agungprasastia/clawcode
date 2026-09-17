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
