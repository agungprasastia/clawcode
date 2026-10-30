use super::*;

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Instant;

#[test]
fn disabled_servers_remain_listed_without_connecting() {
    let configs = BTreeMap::from([(
        "off".to_string(),
        McpServerConfig {
            enabled: false,
            command: vec!["definitely-not-a-real-mcp-command".to_string()],
            ..McpServerConfig::default()
        },
    )]);
    let cwd = std::env::current_dir().unwrap();
    let registry = McpRegistry::new(&configs, &cwd).unwrap();
    assert!(!registry.is_enabled("off"));
    assert!(!registry.is_connected("off"));
    assert!(registry.tools().is_empty());
    assert_eq!(
        registry.statuses(),
        vec![McpServerStatus {
            name: "off".to_string(),
            enabled: false,
            connected: false,
            tool_count: 0,
            error: None,
        }]
    );
}

#[test]
fn sanitizes_server_and_tool_names() {
    assert_eq!(namespace("my server/tools"), "my_server_tools");
    assert_eq!(exposed_name("github", "list issues"), "github_list_issues");
    assert_eq!(exposed_name("a.b", "c.d"), "a_b_c_d");
}

#[test]
fn flattens_tool_result_content_and_structured_fallback() {
    let result = json!({
        "content": [
            {"type": "text", "text": "one"},
            {"type": "resource", "resource": {"uri": "file:///x"}},
            {"type": "image", "mimeType": "image/png"}
        ],
        "structuredContent": {"ignored": true}
    });
    assert_eq!(tool_result_text(&result), "one\nfile:///x\n[image/png]");

    let result = json!({"structuredContent": {"answer": 42}});
    assert_eq!(tool_result_text(&result), "{\"answer\":42}");
}

#[test]
fn parses_json_and_sse_http_bodies() {
    let values = parse_http_body("{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}").unwrap();
    assert_eq!(values.len(), 1);
    assert_eq!(values[0]["id"], 1);

    let values = parse_http_body(
        "event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}\n\n\
         event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{}}\n\n",
    )
    .unwrap();
    assert_eq!(values.len(), 2);
    assert_eq!(values[1]["id"], 2);
}

struct FakeRequest {
    headers: BTreeMap<String, String>,
    body: Value,
}

fn read_request(stream: &mut TcpStream) -> FakeRequest {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut content_length = 0usize;
    let mut headers = BTreeMap::new();
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            let name = name.trim().to_ascii_lowercase();
            let value = value.trim().to_string();
            if name == "content-length" {
                content_length = value.parse().unwrap_or(0);
            }
            headers.insert(name, value);
        }
    }
    let mut body = String::new();
    reader
        .take(content_length as u64)
        .read_to_string(&mut body)
        .unwrap();
    FakeRequest {
        headers,
        body: serde_json::from_str(&body).unwrap(),
    }
}

fn write_response(stream: &mut TcpStream, status: &str, body: &str) {
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes()).unwrap();
}

fn start_fake_server(expected: usize) -> (String, thread::JoinHandle<Vec<FakeRequest>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let handle = thread::spawn(move || {
        let mut requests = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(10);
        while requests.len() < expected && Instant::now() < deadline {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream.set_nonblocking(false).unwrap();
                    let request = read_request(&mut stream);
                    let method = request.body["method"].as_str().unwrap_or_default();
                    let id = request.body["id"].clone();
                    match method {
                        "initialize" => {
                            let body = json!({
                                "jsonrpc": "2.0",
                                "id": id,
                                "result": {
                                    "protocolVersion": PROTOCOL_VERSION,
                                    "capabilities": {"tools": {}},
                                    "serverInfo": {"name": "fake", "version": "1"}
                                }
                            })
                            .to_string();
                            let response = format!(
                                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nMcp-Session-Id: sess-1\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                                body.len()
                            );
                            stream.write_all(response.as_bytes()).unwrap();
                        }
                        "notifications/initialized" => {
                            write_response(&mut stream, "202 Accepted", "");
                        }
                        "tools/list" => {
                            let (tools, cursor) = if request.body["params"].get("cursor").is_some()
                            {
                                (json!([{"name": "beta", "description": "Beta"}]), None)
                            } else {
                                (
                                    json!([{
                                        "name": "alpha",
                                        "description": "Alpha",
                                        "inputSchema": {"type": "object", "properties": {}}
                                    }]),
                                    Some("next"),
                                )
                            };
                            let mut result = json!({"tools": tools});
                            if let Some(cursor) = cursor {
                                result["nextCursor"] = json!(cursor);
                            }
                            let body = json!({"jsonrpc": "2.0", "id": id, "result": result});
                            write_response(&mut stream, "200 OK", &body.to_string());
                        }
                        "tools/call" => {
                            let tool = request.body["params"]["name"].as_str().unwrap_or_default();
                            let body = if tool == "beta" {
                                json!({
                                    "jsonrpc": "2.0",
                                    "id": id,
                                    "result": {
                                        "isError": true,
                                        "content": [{"type": "text", "text": "beta failed"}]
                                    }
                                })
                            } else {
                                json!({
                                    "jsonrpc": "2.0",
                                    "id": id,
                                    "result": {
                                        "content": [{"type": "text", "text": "alpha result"}]
                                    }
                                })
                            };
                            write_response(&mut stream, "200 OK", &body.to_string());
                        }
                        _ => {
                            let body = json!({
                                "jsonrpc": "2.0",
                                "id": id,
                                "error": {"code": -32601, "message": method}
                            });
                            write_response(&mut stream, "200 OK", &body.to_string());
                        }
                    }
                    requests.push(request);
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("fake MCP server accept failed: {error}"),
            }
        }
        requests
    });
    (format!("http://{address}/mcp"), handle)
}

#[test]
fn http_registry_discovers_paginated_tools_calls_and_tracks_session() {
    let (url, server) = start_fake_server(6);
    let mut configs = BTreeMap::new();
    configs.insert(
        "fake".to_string(),
        McpServerConfig {
            enabled: true,
            url: Some(url),
            headers: BTreeMap::from([("Authorization".to_string(), "Bearer test".to_string())]),
            oauth: false,
            ..McpServerConfig::default()
        },
    );
    let cwd = std::env::current_dir().unwrap();
    let registry = McpRegistry::new(&configs, &cwd).unwrap();

    let schemas = registry.tool_schemas();
    assert_eq!(schemas.len(), 2);
    assert_eq!(schemas[0]["function"]["name"], "fake_alpha");
    assert_eq!(schemas[1]["function"]["name"], "fake_beta");
    assert!(registry.has_known_tool("fake_alpha"));
    assert!(!registry.has_known_tool("read_file"));

    let output = registry
        .call_tool("fake_alpha", &json!({"value": 1}))
        .unwrap();
    assert_eq!(output, "alpha result");
    let error = registry.call_tool("fake_beta", &json!({})).unwrap_err();
    assert_eq!(error, "beta failed");

    let status = registry.statuses().pop().unwrap();
    assert_eq!(status.name, "fake");
    assert!(status.enabled);
    assert!(status.connected);
    assert_eq!(status.tool_count, 2);
    assert!(status.error.is_none());
    assert_eq!(registry.probe("fake").unwrap(), 2);
    assert!(registry.disconnect("fake"));
    assert!(!registry.is_enabled("fake"));
    assert!(!registry.is_connected("fake"));
    assert_eq!(registry.tool_count("fake"), 0);
    assert!(registry.status("fake").is_none());
    assert!(registry.tools().is_empty());

    let requests = server.join().unwrap();
    let methods = requests
        .iter()
        .map(|request| request.body["method"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        methods,
        [
            "initialize",
            "notifications/initialized",
            "tools/list",
            "tools/list",
            "tools/call",
            "tools/call"
        ]
    );
    assert_eq!(
        requests[0].headers.get("authorization").map(String::as_str),
        Some("Bearer test")
    );
    assert_eq!(
        requests[2]
            .headers
            .get("mcp-session-id")
            .map(String::as_str),
        Some("sess-1")
    );
    assert_eq!(requests[4].body["params"]["arguments"]["value"], 1);
}

#[derive(Debug)]
struct ToolProvider {
    calls: std::sync::atomic::AtomicUsize,
    tools: std::sync::Arc<Mutex<Vec<String>>>,
}

impl crate::provider::Provider for ToolProvider {
    fn id(&self) -> &crate::provider::ProviderId {
        static ID: std::sync::LazyLock<crate::provider::ProviderId> =
            std::sync::LazyLock::new(|| crate::provider::ProviderId::new("mcp-test"));
        &ID
    }

    fn capabilities(&self) -> crate::provider::ProviderCapabilities {
        crate::provider::ProviderCapabilities {
            streaming: true,
            tools: true,
        }
    }

    fn models(&self) -> Vec<crate::provider::ModelInfo> {
        Vec::new()
    }

    fn send(
        &self,
        request: &crate::provider::StreamRequest,
    ) -> Result<crate::provider::StreamResponse, crate::provider::ProviderError> {
        *self.tools.lock().unwrap() = request
            .tools
            .iter()
            .filter_map(|tool| tool["function"]["name"].as_str().map(str::to_string))
            .collect();
        let call = self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let events = if call == 0 {
            vec![
                crate::provider::StreamEvent::ToolCallStart {
                    id: "call-1".to_string(),
                    name: "fake_alpha".to_string(),
                },
                crate::provider::StreamEvent::ToolCallDelta {
                    id: "call-1".to_string(),
                    arguments: r#"{"value":1}"#.to_string(),
                },
                crate::provider::StreamEvent::ToolCallEnd {
                    id: "call-1".to_string(),
                },
                crate::provider::StreamEvent::Finish {
                    reason: crate::provider::FinishReason::ToolCall,
                },
            ]
        } else {
            vec![
                crate::provider::StreamEvent::TextDelta("done".to_string()),
                crate::provider::StreamEvent::Finish {
                    reason: crate::provider::FinishReason::Stop,
                },
            ]
        };
        Ok(crate::provider::StreamResponse { events })
    }
}

#[test]
fn runtime_executes_mcp_tools_and_advertises_their_schemas() {
    let (url, server) = start_fake_server(5);
    let mut configs = BTreeMap::new();
    configs.insert(
        "fake".to_string(),
        McpServerConfig {
            enabled: true,
            url: Some(url),
            oauth: false,
            ..McpServerConfig::default()
        },
    );
    let cwd = std::env::current_dir().unwrap();
    let registry = McpRegistry::new(&configs, &cwd).unwrap();

    let path = std::env::temp_dir().join(format!(
        "clawcode-mcp-runtime-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let db = crate::persistence::Db::open(&path).unwrap();
    let session = db.create_session("mcp").unwrap();
    let writer =
        crate::persistence::WriterHandle::spawn(crate::persistence::Db::open(&path).unwrap());
    let bus = crate::runtime::EventBus::new();
    let (_subscription, receiver) = bus.subscribe(Some(session.id));
    let provider = ToolProvider {
        calls: std::sync::atomic::AtomicUsize::new(0),
        tools: std::sync::Arc::new(Mutex::new(Vec::new())),
    };
    let tools = std::sync::Arc::clone(&provider.tools);
    let client = crate::runtime::client::RuntimeClient::spawn_with_mcp(
        db,
        writer,
        Box::new(provider),
        bus,
        Some(registry),
    );
    client
        .start_generation(session.id, "build", "mcp-test", "model", "use the mcp tool")
        .unwrap();

    let deadline = Instant::now() + Duration::from_secs(5);
    let mut settled = None;
    while Instant::now() < deadline {
        let Ok(event) = receiver.recv_timeout(Duration::from_millis(50)) else {
            continue;
        };
        if event.kind == "tool_call_settled" {
            settled = Some(serde_json::from_str::<Value>(&event.payload_json).unwrap());
        }
        if event.kind == "generation_finished" {
            break;
        }
    }
    let settled = settled.expect("MCP tool should settle");
    assert_eq!(settled["status"], "completed");
    assert_eq!(settled["result"], "alpha result");
    assert!(tools.lock().unwrap().contains(&"fake_alpha".to_string()));

    let _ = client.shutdown();
    let requests = server.join().unwrap();
    assert_eq!(requests.len(), 5);
    assert_eq!(requests[4].body["params"]["name"], "alpha");
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(path.with_extension("db-wal"));
    let _ = std::fs::remove_file(path.with_extension("db-shm"));
}

#[test]
fn stdio_fixture() {
    if std::env::var("CLAWCODE_MCP_STDIO_FIXTURE").as_deref() != Ok("1") {
        return;
    }
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let Ok(request) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let method = request["method"].as_str().unwrap_or_default();
        let id = request["id"].clone();
        let response = match method {
            "initialize" => Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "protocolVersion": PROTOCOL_VERSION,
                    "capabilities": {"tools": {}},
                    "serverInfo": {"name": "stdio-fixture", "version": "1"}
                }
            })),
            "notifications/initialized" => None,
            "tools/list" => Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "tools": [{
                        "name": "echo",
                        "description": "Echo a message",
                        "inputSchema": {
                            "type": "object",
                            "properties": {"message": {"type": "string"}}
                        }
                    }]
                }
            })),
            "tools/call" => {
                let message = request["params"]["arguments"]["message"]
                    .as_str()
                    .unwrap_or("missing");
                Some(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {"content": [{"type": "text", "text": message}]}
                }))
            }
            _ => Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {"code": -32601, "message": method}
            })),
        };
        if let Some(response) = response {
            writeln!(stdout, "{response}").unwrap();
            stdout.flush().unwrap();
        }
    }
    std::process::exit(0);
}

#[test]
fn stdio_registry_discovers_and_calls_tools() {
    let exe = std::env::current_exe().unwrap();
    let mut configs = BTreeMap::new();
    configs.insert(
        "fixture".to_string(),
        McpServerConfig {
            enabled: true,
            command: vec![
                exe.display().to_string(),
                "--exact".to_string(),
                "mcp::tests::stdio_fixture".to_string(),
            ],
            environment: BTreeMap::from([(
                "CLAWCODE_MCP_STDIO_FIXTURE".to_string(),
                "1".to_string(),
            )]),
            oauth: false,
            ..McpServerConfig::default()
        },
    );
    let cwd = std::env::current_dir().unwrap();
    let registry = McpRegistry::new(&configs, &cwd).unwrap();
    let schemas = registry.tool_schemas();
    assert_eq!(schemas.len(), 1);
    assert_eq!(schemas[0]["function"]["name"], "fixture_echo");
    assert_eq!(
        registry
            .call_tool("fixture_echo", &json!({"message": "hello"}))
            .unwrap(),
        "hello"
    );
}
