//! End-to-end M0 check: one tool call, its result, then a final answer.
//! The mock speaks the Anthropic SSE wire format over plain HTTP; no key ever
//! leaves the test process.

use serde_json::{json, Value};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tagent::{
    core::{self, RunOptions},
    events::Event,
    perm::{Engine, Mode},
    provider::{anthropic::Anthropic, ContentBlock},
    session::{self, SessionStore},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

const KEY: &str = "test-key-not-a-real-secret";

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

async fn read_request(stream: &mut TcpStream) -> (String, String) {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let header_end = loop {
        let n = stream.read(&mut chunk).await.expect("request read failed");
        assert!(n > 0, "provider closed the connection before headers");
        buf.extend_from_slice(&chunk[..n]);
        if let Some(pos) = find(&buf, b"\r\n\r\n") {
            break pos;
        }
    };
    let headers = String::from_utf8_lossy(&buf[..header_end]).into_owned();
    let content_len = headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().expect("bad content-length"))
        })
        .expect("missing content-length");
    let body_start = header_end + 4;
    while buf.len() < body_start + content_len {
        let n = stream
            .read(&mut chunk)
            .await
            .expect("request body read failed");
        assert!(n > 0, "provider closed the connection before the full body");
        buf.extend_from_slice(&chunk[..n]);
    }
    let body = String::from_utf8_lossy(&buf[body_start..body_start + content_len]).into_owned();
    (headers, body)
}

fn frame(value: Value) -> String {
    format!("data: {value}\n\n")
}

fn tool_use_stream() -> String {
    [
        json!({"type": "message_start", "message": {"usage": {"input_tokens": 10}}}),
        json!({"type": "content_block_start", "index": 0, "content_block": {"type": "tool_use", "id": "tu_1", "name": "read", "input": {}}}),
        json!({"type": "content_block_delta", "index": 0, "delta": {"type": "input_json_delta", "partial_json": "{\"path\":\"note.txt\"}"}}),
        json!({"type": "content_block_stop", "index": 0}),
        json!({"type": "message_delta", "delta": {"stop_reason": "tool_use"}, "usage": {"output_tokens": 5}}),
        json!({"type": "message_stop"}),
    ]
    .into_iter()
    .map(frame)
    .collect()
}

fn final_stream() -> String {
    [
        json!({"type": "message_start", "message": {"usage": {"input_tokens": 12}}}),
        json!({"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}}),
        json!({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "done"}}),
        json!({"type": "content_block_stop", "index": 0}),
        json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"}, "usage": {"output_tokens": 3}}),
        json!({"type": "message_stop"}),
    ]
    .into_iter()
    .map(frame)
    .collect()
}

fn temp_workspace() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("ta-e2e-{}-{nanos}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("cannot create temp workspace");
    std::fs::write(dir.join("note.txt"), "hello from file\n").expect("cannot seed note.txt");
    dir
}

#[tokio::test]
async fn agent_loop_reads_a_file_then_answers() {
    let workspace = temp_workspace();
    let session_root = workspace.join("sessions");

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock API");
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let requests: Arc<Mutex<Vec<(String, String)>>> = Arc::new(Mutex::new(Vec::new()));
    let seen = requests.clone();

    let server = tokio::spawn(async move {
        for turn in 0..2 {
            let (mut stream, _) = listener.accept().await.expect("accept provider request");
            let (headers, body) = read_request(&mut stream).await;
            assert!(headers.starts_with("POST /v1/messages"), "{headers}");
            assert!(
                headers
                    .to_ascii_lowercase()
                    .contains(&format!("x-api-key: {KEY}")),
                "BYOK header missing"
            );
            seen.lock().unwrap().push((headers, body));

            let payload = if turn == 0 {
                tool_use_stream()
            } else {
                final_stream()
            };
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{payload}",
                payload.len()
            );
            stream
                .write_all(response.as_bytes())
                .await
                .expect("write mock response");
            stream.flush().await.expect("flush mock response");
        }
    });

    let collected: Arc<Mutex<Vec<Event>>> = Arc::new(Mutex::new(Vec::new()));
    let sink_events = collected.clone();
    let sink: Box<dyn FnMut(Event) + Send> =
        Box::new(move |event| sink_events.lock().unwrap().push(event));
    let provider = Anthropic::new(KEY.to_string(), &base_url).expect("build provider");

    let result = tokio::time::timeout(
        Duration::from_secs(10),
        core::run(RunOptions {
            workspace: workspace.clone(),
            roots: vec![workspace.clone()],
            prompt: "read note.txt".into(),
            model: "anthropic/test-model".into(),
            provider: core::shared(provider),
            permission: Engine::new(Mode::Yolo, Vec::new(), Vec::new()).unwrap(),
            max_turns: 4,
            system_prompt: None,
            append_system_prompt: None,
            session: None,
            session_root: Some(session_root.clone()),
            sink,
            secrets: Vec::new(),
            secret_env_names: Vec::new(),
            permission_reply: None,
            interrupt: None,
            record_sink: None,
            pricing: HashMap::new(),
            compact_threshold: None,
            checkpoints: false,
            project_instructions: Vec::new(),
            skill_prompt: None,
            allowed_tools: None,
            hooks: None,
            extra_tools: Vec::new(),
            depth: 0,
        }),
    )
    .await
    .expect("agent loop timed out")
    .expect("agent loop failed");

    server.await.expect("mock server task failed");

    assert_eq!(result.final_text, "done");
    assert_eq!(result.turns, 2);
    assert_eq!(result.usage.input_tokens, 22);
    assert_eq!(result.usage.output_tokens, 8);

    let events = collected.lock().unwrap();
    assert!(events.iter().any(|event| matches!(
        event,
        Event::ToolResult { name, output, is_error: false, .. }
            if name == "read" && output.contains("hello from file")
    )));
    assert!(!events
        .iter()
        .any(|event| matches!(event, Event::ToolResult { is_error: true, .. })));
    assert!(events.iter().any(|event| matches!(
        event,
        Event::Done { reason, .. } if reason == "end_turn"
    )));

    let requests = requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    let first: Value = serde_json::from_str(&requests[0].1).unwrap();
    assert_eq!(first["messages"][0]["content"][0]["text"], "read note.txt");
    assert_eq!(first["stream"], true);
    let second: Value = serde_json::from_str(&requests[1].1).unwrap();
    let second_text = serde_json::to_string(&second).unwrap();
    assert!(second_text.contains("tool_result"));
    assert!(second_text.contains("hello from file"));
    assert!(second_text.contains("\"tool_use_id\":\"tu_1\""));

    let store = SessionStore::load(session_root.clone(), &result.session_id).unwrap();
    let messages = session::messages(&store.records().unwrap());
    assert!(messages.iter().any(|message| message
        .content
        .iter()
        .any(|block| matches!(block, ContentBlock::ToolUse { id, .. } if id == "tu_1"))));
    assert!(messages.iter().any(|message| message
        .content
        .iter()
        .any(|block| matches!(block, ContentBlock::ToolResult { tool_use_id, .. } if tool_use_id == "tu_1"))));

    std::fs::remove_dir_all(&workspace).unwrap();
}
