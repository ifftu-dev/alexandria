//! Adapter contract tests against a real local HTTP server.

use std::time::Duration;

use alexandria_ai::model::Limits;
use alexandria_ai::{
    connect, Adapter, ChatRequest, Connection, Error, FinishReason, Location, Message, Protocol,
    StreamEvent, Usage,
};
use tokio::net::TcpListener;
use tokio::sync::mpsc;

mod support;
use support::{drain, serve, Reply};

async fn adapter(base: &str, key: Option<&str>) -> Adapter {
    bounded(base, key, Limits::default()).await
}

async fn bounded(base: &str, key: Option<&str>, limits: Limits) -> Adapter {
    let connection = Connection {
        endpoint: base.into(),
        location: Location::Device,
        protocol: Protocol::Compatible,
    };
    connect(&connection, key, limits).await.unwrap()
}

fn request() -> ChatRequest {
    ChatRequest {
        model: "llama3.1:8b".into(),
        messages: vec![Message::system("Be brief."), Message::user("Hello")],
        max_output_tokens: 256,
        temperature: None,
    }
}

#[tokio::test]
async fn streams_deltas_across_split_utf8_and_reports_usage() {
    let body = "data: {\"choices\":[{\"delta\":{\"content\":\"Bonjour é\"}}]}\n\n\
                data: {\"choices\":[{\"delta\":{\"content\":\"漢字\"},\"finish_reason\":\"stop\"}]}\n\n\
                data: {\"choices\":[],\"usage\":{\"prompt_tokens\":12,\"completion_tokens\":4}}\n\n\
                data: [DONE]\n\n"
        .as_bytes();
    // Split inside the two-byte "é" and inside the three-byte "漢".
    let e = body.windows(2).position(|w| w == "é".as_bytes()).unwrap() + 1;
    let k = body.windows(3).position(|w| w == "漢".as_bytes()).unwrap() + 2;
    let (base, seen) = serve(Reply::sse(&[&body[..e], &body[e..k], &body[k..]])).await;
    let (tx, rx) = mpsc::channel(4);
    let events = tokio::spawn(drain(rx));
    let completion = adapter(&base, Some("sk-test"))
        .await
        .stream(&request(), tx)
        .await
        .unwrap();
    assert_eq!(completion.text, "Bonjour é漢字");
    assert_eq!(completion.finish, FinishReason::Stop);
    let usage = Usage {
        input_tokens: Some(12),
        output_tokens: Some(4),
    };
    assert_eq!(completion.usage, Some(usage));
    assert_eq!(
        events.await.unwrap(),
        vec![
            StreamEvent::TextDelta {
                text: "Bonjour é".into()
            },
            StreamEvent::TextDelta {
                text: "漢字".into()
            },
            StreamEvent::Usage { usage },
            StreamEvent::Finished {
                reason: FinishReason::Stop
            },
        ]
    );
    let raw = seen.await.unwrap();
    assert!(raw.starts_with("POST /v1/chat/completions"));
    assert!(
        raw.contains("authorization: Bearer sk-test")
            || raw.contains("Authorization: Bearer sk-test")
    );
    assert!(raw.contains("\"stream\":true"));
    assert!(raw.contains("\"max_tokens\":256"));
    assert!(!raw.contains("stream_options"));
}

#[tokio::test]
async fn a_stream_cut_off_without_a_finish_is_interrupted_not_complete() {
    let (base, _) = serve(Reply::sse(&[
        b"data: {\"choices\":[{\"delta\":{\"content\":\"Half an answ\"}}]}\n\n",
    ]))
    .await;
    let (tx, rx) = mpsc::channel(4);
    let events = tokio::spawn(drain(rx));
    let error = adapter(&base, None)
        .await
        .stream(&request(), tx)
        .await
        .unwrap_err();
    assert_eq!(error, Error::Interrupted);
    assert!(error.may_have_been_accepted());
    // The partial text was still delivered, so the caller can keep it.
    assert_eq!(
        events.await.unwrap(),
        vec![StreamEvent::TextDelta {
            text: "Half an answ".into()
        }]
    );
}

#[tokio::test]
async fn a_finish_reason_without_done_is_accepted() {
    let (base, _) = serve(Reply::sse(&[
        b"data: {\"choices\":[{\"delta\":{\"content\":\"ok\"},\"finish_reason\":\"length\"}]}\n\n",
    ]))
    .await;
    let (tx, rx) = mpsc::channel(4);
    tokio::spawn(drain(rx));
    let completion = adapter(&base, None)
        .await
        .stream(&request(), tx)
        .await
        .unwrap();
    assert_eq!(completion.finish, FinishReason::Length);
    assert_eq!(completion.usage, None);
}

#[tokio::test]
async fn a_server_that_ignores_streaming_still_answers() {
    let (base, _) = serve(Reply::json(
        "200 OK",
        "",
        r#"{"choices":[{"message":{"content":"Whole answer"},"finish_reason":"stop"}]}"#,
    ))
    .await;
    let (tx, rx) = mpsc::channel(4);
    let events = tokio::spawn(drain(rx));
    let completion = adapter(&base, None)
        .await
        .stream(&request(), tx)
        .await
        .unwrap();
    assert_eq!(completion.text, "Whole answer");
    assert_eq!(events.await.unwrap().len(), 2);
}

#[tokio::test]
async fn error_bodies_and_keys_never_reach_the_message() {
    let (base, _) = serve(Reply::json(
        "401 Unauthorized",
        "",
        r#"{"error":"invalid key sk-secret-123 for org acme"}"#,
    ))
    .await;
    let error = adapter(&base, Some("sk-secret-123"))
        .await
        .complete(&request())
        .await
        .unwrap_err();
    assert!(error.is_auth());
    let text = error.to_string();
    assert!(text.contains("401"));
    assert!(!text.contains("sk-secret") && !text.contains("acme"));
    assert!(!error.may_have_been_accepted());
}

#[tokio::test]
async fn rate_limits_report_retry_after_and_redirects_are_not_followed() {
    let (base, _) = serve(Reply::json(
        "429 Too Many Requests",
        "Retry-After: 7\r\n",
        "{}",
    ))
    .await;
    let error = adapter(&base, None)
        .await
        .complete(&request())
        .await
        .unwrap_err();
    assert_eq!(
        error,
        Error::Http {
            status: 429,
            retry_after: Some(7)
        }
    );

    // A redirect to another listener must not be followed: the key and the
    // request body would go somewhere the user never configured.
    let elsewhere = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let target = format!("http://{}/steal", elsewhere.local_addr().unwrap());
    let (base, _) = serve(Reply::json(
        "307 Temporary Redirect",
        &format!("Location: {target}\r\n"),
        "",
    ))
    .await;
    let error = adapter(&base, Some("sk-test"))
        .await
        .complete(&request())
        .await
        .unwrap_err();
    assert!(matches!(error, Error::Http { status: 307, .. }));
    assert!(
        tokio::time::timeout(Duration::from_millis(200), elsewhere.accept())
            .await
            .is_err(),
        "redirect target was contacted"
    );
}

#[tokio::test]
async fn output_beyond_the_limit_is_refused() {
    let big = format!(
        "data: {{\"choices\":[{{\"delta\":{{\"content\":\"{}\"}}}}]}}\n\n",
        "x".repeat(600)
    );
    let (base, _) = serve(Reply::sse(&[big.as_bytes(), big.as_bytes()])).await;
    let (tx, rx) = mpsc::channel(4);
    tokio::spawn(drain(rx));
    let limits = Limits {
        max_output_bytes: 1000,
        ..Limits::default()
    };
    let error = bounded(&base, None, limits)
        .await
        .stream(&request(), tx)
        .await
        .unwrap_err();
    assert_eq!(error, Error::TooLarge);
}

#[tokio::test]
async fn dropping_the_receiver_cancels_the_stream() {
    let (base, _) = serve(Reply::sse(&[
        b"data: {\"choices\":[{\"delta\":{\"content\":\"one\"}}]}\n\n",
        b"data: {\"choices\":[{\"delta\":{\"content\":\"two\"}}]}\n\n",
    ]))
    .await;
    let (tx, rx) = mpsc::channel(1);
    drop(rx);
    let error = adapter(&base, None)
        .await
        .stream(&request(), tx)
        .await
        .unwrap_err();
    assert_eq!(error, Error::Cancelled);
}

#[tokio::test]
async fn a_silent_server_times_out() {
    let mut reply = Reply::sse(&[]);
    reply.body = vec![(
        Duration::from_secs(2),
        b"data: {\"choices\":[{\"delta\":{\"content\":\"late\"}}]}\n\n".to_vec(),
    )];
    let (base, _) = serve(reply).await;
    let (tx, rx) = mpsc::channel(4);
    tokio::spawn(drain(rx));
    let limits = Limits {
        idle_timeout: Duration::from_millis(150),
        ..Limits::default()
    };
    let error = bounded(&base, None, limits)
        .await
        .stream(&request(), tx)
        .await
        .unwrap_err();
    assert_eq!(error, Error::Timeout);
}

#[tokio::test]
async fn an_error_frame_mid_stream_stops_with_a_provider_error() {
    let (base, _) = serve(Reply::sse(&[
        b"data: {\"choices\":[{\"delta\":{\"content\":\"Start\"}}]}\n\n",
        b"data: {\"error\":{\"message\":\"overloaded: internal detail\"}}\n\n",
    ]))
    .await;
    let (tx, rx) = mpsc::channel(4);
    tokio::spawn(drain(rx));
    let error = adapter(&base, None)
        .await
        .stream(&request(), tx)
        .await
        .unwrap_err();
    assert_eq!(error, Error::Provider);
    assert!(!error.to_string().contains("internal detail"));
}

#[tokio::test]
async fn malformed_frames_are_rejected() {
    let (base, _) = serve(Reply::sse(&[b"data: {not json\n\n"])).await;
    let (tx, rx) = mpsc::channel(4);
    tokio::spawn(drain(rx));
    let error = adapter(&base, None)
        .await
        .stream(&request(), tx)
        .await
        .unwrap_err();
    assert_eq!(error, Error::Malformed);
}

#[tokio::test]
async fn non_streamed_completion_and_empty_answers() {
    let (base, _) = serve(Reply::json(
        "200 OK",
        "",
        r#"{"choices":[{"message":{"content":"Hi"}}],"usage":{"prompt_tokens":3}}"#,
    ))
    .await;
    let completion = adapter(&base, None)
        .await
        .complete(&request())
        .await
        .unwrap();
    assert_eq!(completion.text, "Hi");
    assert_eq!(completion.finish, FinishReason::Unknown);
    assert_eq!(
        completion.usage,
        Some(Usage {
            input_tokens: Some(3),
            output_tokens: None
        })
    );

    let (base, _) = serve(Reply::json(
        "200 OK",
        "",
        r#"{"choices":[{"message":{"content":"   "}}]}"#,
    ))
    .await;
    assert_eq!(
        adapter(&base, None)
            .await
            .complete(&request())
            .await
            .unwrap_err(),
        Error::Empty
    );
}

#[tokio::test]
async fn model_discovery_lists_ids_or_reports_unsupported() {
    let (base, seen) = serve(Reply::json(
        "200 OK",
        "",
        r#"{"data":[{"id":"qwen3-32b"},{"id":"llama3.1:8b"},{"id":"qwen3-32b"}]}"#,
    ))
    .await;
    let ids = adapter(&base, None).await.list_models().await.unwrap();
    assert_eq!(
        ids,
        vec!["llama3.1:8b".to_string(), "qwen3-32b".to_string()]
    );
    assert!(seen.await.unwrap().starts_with("GET /v1/models"));

    let (base, _) = serve(Reply::json("404 Not Found", "", "{}")).await;
    assert_eq!(
        adapter(&base, None).await.list_models().await.unwrap_err(),
        Error::Unsupported("model discovery")
    );
}

#[tokio::test]
async fn debug_output_redacts_the_key() {
    let a = adapter("http://localhost:9/v1", Some("sk-very-secret")).await;
    let printed = format!("{a:?}");
    assert!(!printed.contains("sk-very-secret"));
    assert!(printed.contains("has_key: true"));
}

#[tokio::test]
async fn an_unreachable_endpoint_was_never_accepted() {
    // Bind then drop to get a port with nothing listening.
    let port = {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        l.local_addr().unwrap().port()
    };
    let error = adapter(&format!("http://127.0.0.1:{port}/v1"), None)
        .await
        .complete(&request())
        .await
        .unwrap_err();
    assert_eq!(error, Error::Unreachable);
    assert!(!error.may_have_been_accepted());
}
