//! MCP 2026-07-28 conformance tests for the stdio server, driven by raw
//! JSON-RPC (independent of rmcp's client) and validated against the pinned
//! specification schema in `tests/schema/` (see its NOTICE for provenance).

use std::process::Stdio;
use std::sync::OnceLock;
use std::time::Duration;

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

const SCHEMA: &str = include_str!("schema/mcp-2026-07-28.schema.json");
const MODERN: &str = "2026-07-28";

fn schema() -> &'static jsonschema::ValidatorMap {
    static MAP: OnceLock<jsonschema::ValidatorMap> = OnceLock::new();
    MAP.get_or_init(|| {
        jsonschema::validator_map_for(&serde_json::from_str(SCHEMA).unwrap()).unwrap()
    })
}

/// Asserts `instance` satisfies a definition of the pinned 2026-07-28 schema.
fn assert_schema(definition: &str, instance: &Value) {
    let map = schema();
    let validator = map
        .get(&format!("#/$defs/{definition}"))
        .or_else(|| map.get(&format!("/$defs/{definition}")))
        .unwrap_or_else(|| panic!("pinned schema has no {definition}"));
    let errors: Vec<String> = validator
        .iter_errors(instance)
        .map(|error| error.to_string())
        .collect();
    assert!(
        errors.is_empty(),
        "{definition} violations: {errors:?}\n{instance}"
    );
}

fn meta() -> Value {
    json!({
        "io.modelcontextprotocol/protocolVersion": MODERN,
        "io.modelcontextprotocol/clientInfo": {"name": "conformance", "version": "1.0.0"},
        "io.modelcontextprotocol/clientCapabilities": {},
    })
}

fn request(id: i64, method: &str, mut params: Value) -> Value {
    params["_meta"] = meta();
    json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
}

struct Server {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: Lines<BufReader<ChildStdout>>,
}

impl Server {
    fn spawn(connection_file: Option<&std::path::Path>) -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_alexandria-mcp"));
        command
            .env_remove("ALEXANDRIA_MCP_CONNECTION_FILE")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        if let Some(file) = connection_file {
            command.env("ALEXANDRIA_MCP_CONNECTION_FILE", file);
        }
        let mut child = command.spawn().unwrap();
        let stdin = child.stdin.take();
        let stdout = BufReader::new(child.stdout.take().unwrap()).lines();
        Self {
            child,
            stdin,
            stdout,
        }
    }

    async fn send_line(&mut self, line: &str) {
        let stdin = self.stdin.as_mut().unwrap();
        stdin.write_all(line.as_bytes()).await.unwrap();
        stdin.write_all(b"\n").await.unwrap();
        stdin.flush().await.unwrap();
    }

    async fn send(&mut self, message: Value) {
        self.send_line(&message.to_string()).await;
    }

    async fn recv_unchecked(&mut self) -> Value {
        let line = tokio::time::timeout(Duration::from_secs(5), self.stdout.next_line())
            .await
            .expect("server response timed out")
            .unwrap()
            .expect("server closed stdout");
        serde_json::from_str(&line).expect("stdout must carry only JSON-RPC messages")
    }

    /// Next message, which must be a valid 2026-07-28 JSON-RPC message.
    async fn recv(&mut self) -> Value {
        let message = self.recv_unchecked().await;
        assert_schema("JSONRPCMessage", &message);
        message
    }

    async fn request(&mut self, id: i64, method: &str, params: Value) -> Value {
        self.send(request(id, method, params)).await;
        let response = self.recv().await;
        assert_eq!(response["id"], id, "{response}");
        response
    }

    async fn silent_for(&mut self, duration: Duration) -> bool {
        tokio::time::timeout(duration, self.stdout.next_line())
            .await
            .is_err()
    }
}

fn error_code(response: &Value) -> i64 {
    assert_schema("JSONRPCErrorResponse", response);
    response["error"]["code"].as_i64().unwrap()
}

fn server_name(result: &Value) -> &Value {
    &result["_meta"]["io.modelcontextprotocol/serverInfo"]["name"]
}

/// JSON pointers of `type` arrays, which several MCP clients misread.
fn type_arrays(schema: &Value, path: String, found: &mut Vec<String>) {
    match schema {
        Value::Object(map) => {
            if map.get("type").is_some_and(Value::is_array) {
                found.push(path.clone());
            }
            for (key, value) in map {
                type_arrays(value, format!("{path}/{key}"), found);
            }
        }
        Value::Array(items) => {
            for (index, value) in items.iter().enumerate() {
                type_arrays(value, format!("{path}/{index}"), found);
            }
        }
        _ => {}
    }
}

#[test]
#[should_panic(expected = "DiscoverResultResponse violations")]
fn pinned_schema_rejects_nonconforming_results() {
    // A discovery result without the required resultType must not validate.
    assert_schema(
        "DiscoverResultResponse",
        &json!({"jsonrpc": "2.0", "id": 1, "result": {"supportedVersions": [MODERN], "capabilities": {}, "ttlMs": 0, "cacheScope": "private"}}),
    );
}

#[tokio::test]
async fn discovery_versions_and_request_metadata() {
    let mut server = Server::spawn(None);
    let discovery = server.request(1, "server/discover", json!({})).await;
    assert_schema("DiscoverResultResponse", &discovery);
    let supported = discovery["result"]["supportedVersions"].clone();
    assert!(supported.as_array().unwrap().iter().any(|v| v == MODERN));
    assert_eq!(server_name(&discovery["result"]), "alexandria-mcp");
    assert_eq!(discovery["result"]["capabilities"], json!({"tools": {}}));

    for (id, method) in [(2, "server/discover"), (3, "tools/list")] {
        server
            .send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": {}}))
            .await;
        let response = server.recv().await;
        assert_eq!(response["id"], id);
        assert_eq!(error_code(&response), -32602, "missing _meta on {method}");
    }
    server
        .send(json!({"jsonrpc": "2.0", "id": 4, "method": "tools/list", "params": {"_meta": {"io.modelcontextprotocol/protocolVersion": MODERN}}}))
        .await;
    assert_eq!(
        error_code(&server.recv().await),
        -32602,
        "clientCapabilities is required"
    );

    let mut unsupported = request(5, "tools/list", json!({}));
    unsupported["params"]["_meta"]["io.modelcontextprotocol/protocolVersion"] = json!("1999-01-01");
    server.send(unsupported).await;
    let response = server.recv().await;
    assert_schema("UnsupportedProtocolVersionError", &response);
    assert_eq!(response["id"], 5);
    assert_eq!(response["error"]["data"]["requested"], "1999-01-01");
    assert_eq!(response["error"]["data"]["supported"], supported);

    let mut string_id = request(0, "server/discover", json!({}));
    string_id["id"] = json!("discover-string");
    server.send(string_id).await;
    let response = server.recv().await;
    assert_eq!(response["id"], "discover-string");
    assert_schema("DiscoverResultResponse", &response);
}

#[tokio::test]
async fn tools_list_and_call_conform_to_schema() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("grant.json");
    std::fs::write(
        &file,
        json!({"socket": dir.path().join("missing.sock"), "token": "unused"}).to_string(),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
    }

    for (connection, expected) in [(None, 1), (Some(file.as_path()), 11)] {
        let mut server = Server::spawn(connection);
        let first = server.request(1, "tools/list", json!({})).await;
        assert_schema("ListToolsResultResponse", &first);
        let result = &first["result"];
        assert_eq!(result["cacheScope"], "private");
        assert_eq!(result["ttlMs"], 0);
        assert_eq!(server_name(result), "alexandria-mcp");
        let tools = result["tools"].as_array().unwrap();
        assert_eq!(tools.len(), expected);
        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        assert_eq!(names, sorted, "deterministic order");
        for tool in tools {
            let name = tool["name"].as_str().unwrap();
            assert!(
                (1..=128).contains(&name.len())
                    && name
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_-.".contains(&b)),
                "tool name {name}"
            );
            jsonschema::draft202012::meta::validate(&tool["inputSchema"]).unwrap();
            jsonschema::draft202012::meta::validate(&tool["outputSchema"]).unwrap();
            let mut found = Vec::new();
            type_arrays(tool, name.to_string(), &mut found);
            assert!(found.is_empty(), "type arrays in tool schemas: {found:?}");
        }
        let second = server.request(2, "tools/list", json!({})).await;
        assert_eq!(second["result"]["tools"], result["tools"]);
    }

    let mut server = Server::spawn(None);
    let listing = server.request(1, "tools/list", json!({})).await;
    let output_schema = listing["result"]["tools"][0]["outputSchema"].clone();
    let vector: Value = serde_json::from_str(include_str!(
        "../../alexandria-verify/tests/vectors/01-valid.json"
    ))
    .unwrap();
    let call = server
        .request(
            2,
            "tools/call",
            json!({"name": "verify_credential", "arguments": {"credential_json": vector["credential"].to_string()}}),
        )
        .await;
    assert_schema("CallToolResultResponse", &call);
    let result = &call["result"];
    assert_eq!(result["isError"], false);
    assert_eq!(server_name(result), "alexandria-mcp");
    let structured = &result["structuredContent"];
    let output = jsonschema::validator_for(&output_schema).unwrap();
    assert!(output.is_valid(structured), "{structured}");
    let text: Value = serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(&text, structured, "text content mirrors structured content");

    let failed = server
        .request(
            3,
            "tools/call",
            json!({"name": "verify_credential", "arguments": {"credential_json": "not JSON"}}),
        )
        .await;
    assert_schema("CallToolResultResponse", &failed);
    assert_eq!(failed["result"]["isError"], true);

    let unknown = server
        .request(
            4,
            "tools/call",
            json!({"name": "publish_course", "arguments": {}}),
        )
        .await;
    assert_eq!(error_code(&unknown), -32602);
}

#[tokio::test]
async fn unadvertised_and_removed_methods_are_not_found() {
    let mut server = Server::spawn(None);
    for (id, method, params) in [
        (1, "resources/list", json!({})),
        (2, "resources/templates/list", json!({})),
        (3, "resources/read", json!({"uri": "alexandria://course"})),
        (4, "prompts/list", json!({})),
        (5, "prompts/get", json!({"name": "study"})),
        (
            6,
            "completion/complete",
            json!({"ref": {"type": "ref/prompt", "name": "study"}, "argument": {"name": "a", "value": "b"}}),
        ),
        (7, "logging/setLevel", json!({"level": "info"})),
        (8, "ping", json!({})),
        (9, "example/unknown", json!({})),
    ] {
        let response = server.request(id, method, params).await;
        assert_eq!(error_code(&response), -32601, "{method}: {response}");
    }
}

#[tokio::test]
async fn invalid_messages_are_rejected_without_ending_the_session() {
    let mut server = Server::spawn(None);
    for (line, code, id) in [
        ("{not json".to_string(), -32700, Value::Null),
        (
            format!("[{}]", request(1, "server/discover", json!({}))),
            -32600,
            Value::Null,
        ),
        (
            json!({"jsonrpc": "1.0", "id": 2, "method": "server/discover", "params": {"_meta": meta()}}).to_string(),
            -32600,
            json!(2),
        ),
        (
            json!({"jsonrpc": "2.0", "id": null, "method": "server/discover", "params": {"_meta": meta()}}).to_string(),
            -32600,
            Value::Null,
        ),
    ] {
        server.send_line(&line).await;
        let response = server.recv().await;
        assert_eq!(error_code(&response), code, "{line}");
        assert_eq!(response.get("id").cloned().unwrap_or(Value::Null), id);
    }
    // Notifications, including malformed ones, never receive responses.
    server.send_line("").await;
    server
        .send(json!({"jsonrpc": "2.0", "method": "notifications/cancelled", "params": {}}))
        .await;
    server
        .send(json!({"jsonrpc": "2.0", "method": "notifications/cancelled", "params": {"requestId": 99}}))
        .await;
    assert!(server.silent_for(Duration::from_millis(300)).await);
    let discovery = server.request(3, "server/discover", json!({})).await;
    assert_schema("DiscoverResultResponse", &discovery);
}

#[cfg(unix)]
#[tokio::test]
async fn cancelled_requests_receive_no_response() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("broker.sock");
    let listener = tokio::net::UnixListener::bind(&socket).unwrap();
    let file = dir.path().join("grant.json");
    std::fs::write(
        &file,
        json!({"socket": socket, "token": "token"}).to_string(),
    )
    .unwrap();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
    // A slow broker: each draft read completes 800 ms after it arrives.
    let broker = tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            tokio::spawn(async move {
                let (reader, mut writer) = stream.into_split();
                let mut line = String::new();
                BufReader::new(reader).read_line(&mut line).await.unwrap();
                tokio::time::sleep(Duration::from_millis(800)).await;
                let result = json!({"result": {"course_id": "course", "element_id": "lesson", "title": "Lesson", "text": "Draft", "fingerprint": "f", "audience": "", "outcome": "", "initial_prompt": "", "sources": []}});
                let _ = writer.write_all(format!("{result}\n").as_bytes()).await;
            });
        }
    });
    let mut server = Server::spawn(Some(&file));
    let read = json!({"name": "read_lesson_draft", "arguments": {"course_id": "course", "element_id": "lesson"}});

    let completed = server.request(1, "tools/call", read.clone()).await;
    assert_schema("CallToolResultResponse", &completed);

    server.send(request(2, "tools/call", read)).await;
    server
        .send(json!({"jsonrpc": "2.0", "method": "notifications/cancelled", "params": {"requestId": 2, "reason": "conformance"}}))
        .await;
    let discovery = server.request(3, "server/discover", json!({})).await;
    assert_schema("DiscoverResultResponse", &discovery);
    assert!(
        server.silent_for(Duration::from_millis(2000)).await,
        "no message may follow cancellation of request 2"
    );
    broker.abort();
}

#[tokio::test]
async fn messages_before_the_first_request_do_not_end_the_session() {
    for first in [
        json!({"jsonrpc": "2.0", "method": "notifications/cancelled", "params": {"requestId": 1}}),
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        json!({"jsonrpc": "2.0", "id": 1, "result": {"resultType": "complete"}}),
    ] {
        let mut server = Server::spawn(None);
        server.send(first.clone()).await;
        assert!(
            server.silent_for(Duration::from_millis(200)).await,
            "{first}"
        );
        let discovery = server.request(2, "server/discover", json!({})).await;
        assert_schema("DiscoverResultResponse", &discovery);
    }
}

#[tokio::test]
async fn stdin_eof_ends_the_process_cleanly() {
    let mut server = Server::spawn(None);
    server.request(1, "server/discover", json!({})).await;
    drop(server.stdin.take());
    let status = tokio::time::timeout(Duration::from_secs(5), server.child.wait())
        .await
        .expect("server must exit after stdin closes")
        .unwrap();
    assert!(status.success());
    assert!(server.stdout.next_line().await.unwrap().is_none());
}

#[tokio::test]
async fn each_advertised_legacy_revision_initializes_independently() {
    let mut discovery = Server::spawn(None);
    let supported = discovery.request(1, "server/discover", json!({})).await["result"]
        ["supportedVersions"]
        .as_array()
        .unwrap()
        .clone();
    let legacy: Vec<&str> = supported
        .iter()
        .filter_map(Value::as_str)
        .filter(|version| *version != MODERN)
        .collect();
    assert!(!legacy.is_empty());
    for version in legacy {
        let mut server = Server::spawn(None);
        server
            .send(json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": version, "capabilities": {}, "clientInfo": {"name": "legacy", "version": "1.0.0"}}}))
            .await;
        let initialized = server.recv_unchecked().await;
        assert_eq!(initialized["id"], 1);
        assert_eq!(
            initialized["result"]["protocolVersion"], version,
            "{initialized}"
        );
        assert!(initialized["result"].get("resultType").is_none());
        server
            .send(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))
            .await;
        server
            .send(json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}}))
            .await;
        let listing = server.recv_unchecked().await;
        assert_eq!(listing["id"], 2);
        let result = &listing["result"];
        assert_eq!(result["tools"][0]["name"], "verify_credential");
        for modern_only in ["resultType", "ttlMs", "cacheScope"] {
            assert!(
                result.get(modern_only).is_none(),
                "{version} must not use {modern_only}: {listing}"
            );
        }
    }
}
