use clawcode::adapters::{
    MockTransport, anthropic::Anthropic, ollama::Ollama, openai_compatible::OpenAiCompatible,
};
use clawcode::provider::{Provider, StreamRequest};

fn request() -> StreamRequest {
    StreamRequest {
        model: "mock".into(),
        prompt: "hello".into(),
        max_output_tokens: 32,
    }
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
