use clawcode::config::ConfigLoader;
use clawcode::provider::{
    MAX_COALESCED_DELTA_BYTES, ProviderStream, StreamEvent, ToolCallAssembler,
};
use clawcode::tui::{UiEvent, UiEventQueue};
use clawcode::workspace::WorkspaceRoot;
use std::fs;
use std::path::PathBuf;

fn generated_input(seed: u64, length: usize) -> String {
    let mut state = seed;
    (0..length)
        .map(|_| {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            char::from(b' ' + (state as u8 % 95))
        })
        .collect()
}

#[test]
fn generated_config_inputs_never_panic() {
    let loader = ConfigLoader;
    for seed in 0..128 {
        let source = generated_input(seed, (seed as usize * 17) % 257);
        let _ = loader.parse_str("generated.jsonc", &source);
    }
}

#[test]
fn generated_tool_fragments_never_exceed_cap() {
    let mut assembler = ToolCallAssembler::new(256);
    for seed in 0..128 {
        let fragment = generated_input(seed, 1 + (seed as usize % 31));
        let _ = assembler.push("call", &fragment);
        assert!(assembler.finish("call").unwrap_or_default().len() <= 256);
    }
}

#[test]
fn generated_stream_and_ui_deltas_stay_bounded() {
    let (sender, mut stream) = ProviderStream::channel(1024);
    for seed in 0..512 {
        sender
            .send(StreamEvent::TextDelta(generated_input(seed, 257)))
            .unwrap();
    }
    sender.flush().unwrap();
    drop(sender);
    while let Some(StreamEvent::TextDelta(delta)) = stream.next() {
        assert!(delta.len() <= MAX_COALESCED_DELTA_BYTES);
    }

    let mut queue = UiEventQueue::new(8);
    for seed in 0..512 {
        queue.push(UiEvent::StreamDelta(generated_input(seed, 257)));
        assert!(queue.len() <= 8);
    }
}

#[test]
fn generated_workspace_traversal_is_rejected() {
    let root = unique_temp_dir();
    fs::create_dir_all(&root).unwrap();
    let workspace = WorkspaceRoot::open(&root).unwrap();
    for seed in 0..128 {
        let path = format!("nested/{}/../../escape-{seed}", generated_input(seed, 3));
        assert!(workspace.resolve(path).is_err());
    }
    fs::remove_dir_all(root).unwrap();
}

fn unique_temp_dir() -> PathBuf {
    std::env::temp_dir().join(format!("clawcode-m8-property-{}", std::process::id()))
}
