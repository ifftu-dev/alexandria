use rmcp::{model::CallToolRequestParams, transport::TokioChildProcess, ServiceExt};
use serde_json::json;

#[tokio::test]
async fn actual_stdio_binary_verifies_good_bad_and_malformed_credentials() {
    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_alexandria-mcp"));
    command.env_remove("ALEXANDRIA_MCP_CONNECTION_FILE");
    let process = TokioChildProcess::new(command).unwrap();
    let client = ().serve(process).await.unwrap();
    let tools = client.list_all_tools().await.unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "verify_credential");
    assert!(tools[0].output_schema.is_some());
    for (fixture, valid) in [
        (
            include_str!("../../alexandria-verify/tests/vectors/01-valid.json"),
            true,
        ),
        (
            include_str!("../../alexandria-verify/tests/vectors/02-tampered-payload.json"),
            false,
        ),
    ] {
        let value: serde_json::Value = serde_json::from_str(fixture).unwrap();
        let args = json!({"credential_json":value["credential"].to_string()})
            .as_object()
            .unwrap()
            .clone();
        let result = client
            .call_tool(CallToolRequestParams::new("verify_credential").with_arguments(args))
            .await
            .unwrap();
        assert!(!result.is_error.unwrap_or(false));
        let body = result.structured_content.unwrap();
        assert_eq!(body["signature_valid"], valid);
        assert_eq!(body["revocation_status"], "unknown");
        assert_eq!(body["accreditation_status"], "unknown");
    }
    let result = client
        .call_tool(
            CallToolRequestParams::new("verify_credential").with_arguments(
                json!({"credential_json":"not JSON"})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await
        .unwrap();
    assert!(result.is_error.unwrap_or(false));
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn oversized_stdio_frame_is_bounded_and_process_stops() {
    use std::process::Stdio;
    use tokio::io::AsyncWriteExt;
    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_alexandria-mcp"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let _ = stdin.write_all(&vec![b'x'; 262_145]).await;
    drop(stdin);
    let status = tokio::time::timeout(std::time::Duration::from_secs(5), child.wait()).await;
    assert!(status.is_ok());
}

#[cfg(unix)]
#[tokio::test]
async fn configured_stdio_client_uses_broker_and_propagates_revocation() {
    use std::os::unix::fs::PermissionsExt;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("broker.sock");
    let listener = tokio::net::UnixListener::bind(&socket).unwrap();
    let file = dir.path().join("grant.json");
    std::fs::write(
        &file,
        json!({"socket":socket,"token":"test-private-token"}).to_string(),
    )
    .unwrap();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
    let server = tokio::spawn(async move {
        for allowed in [true, false] {
            let (stream, _) = listener.accept().await.unwrap();
            let (reader, mut writer) = stream.into_split();
            let mut reader = tokio::io::BufReader::new(reader);
            let mut line = String::new();
            reader.read_line(&mut line).await.unwrap();
            let request: serde_json::Value = serde_json::from_str(&line).unwrap();
            assert_eq!(request["token"], "test-private-token");
            assert_eq!(request["operation"], "read_lesson_draft");
            let response = if allowed {
                json!({"result":{"course_id":"mine","element_id":"lesson","title":"Lesson","text":"Lesson draft","fingerprint":"test","audience":"Beginners","outcome":"Learn","initial_prompt":"Be clear","sources":[]}})
            } else {
                json!({"error":"permission_denied"})
            };
            writer
                .write_all(format!("{response}\n").as_bytes())
                .await
                .unwrap();
        }
    });
    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_alexandria-mcp"));
    command.env("ALEXANDRIA_MCP_CONNECTION_FILE", &file);
    let client = ().serve(TokioChildProcess::new(command).unwrap()).await.unwrap();
    let tools = client.list_all_tools().await.unwrap();
    assert_eq!(tools.len(), 11);
    assert!(!tools
        .iter()
        .any(|t| t.name.contains("publish") || t.name.contains("apply")));
    for allowed in [true, false] {
        let result = client
            .call_tool(
                CallToolRequestParams::new("read_lesson_draft").with_arguments(
                    json!({"course_id":"mine","element_id":"lesson"})
                        .as_object()
                        .unwrap()
                        .clone(),
                ),
            )
            .await
            .unwrap();
        assert_eq!(result.is_error.unwrap_or(false), !allowed);
        assert!(!serde_json::to_string(&result)
            .unwrap()
            .contains("test-private-token"));
        if allowed {
            assert_eq!(result.structured_content.unwrap()["text"], "Lesson draft");
        }
    }
    server.await.unwrap();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
    let denied = client
        .call_tool(CallToolRequestParams::new("list_course_drafts"))
        .await
        .unwrap();
    assert!(denied.is_error.unwrap_or(false));
    client.cancel().await.unwrap();
}
