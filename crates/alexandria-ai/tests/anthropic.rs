//! Messages protocol contract tests against a real local HTTP server.

use alexandria_ai::anthropic::{API_VERSION, THINKING_HEADROOM};
use alexandria_ai::model::Limits;
use alexandria_ai::{
    connect, Adapter, ChatRequest, Connection, Error, FinishReason, Location, Message, Protocol,
    StreamEvent, Usage,
};
use tokio::sync::mpsc;

mod support;
use support::{drain, serve, Reply};

async fn adapter(base: &str, key: Option<&str>) -> Adapter {
    let connection = Connection {
        endpoint: base.into(),
        location: Location::Device,
        protocol: Protocol::Anthropic,
    };
    connect(&connection, key, Limits::default()).await.unwrap()
}

fn request() -> ChatRequest {
    ChatRequest {
        model: "claude-opus-5-5".into(),
        messages: vec![
            Message::system("Be brief."),
            Message::user("Hello"),
            Message::assistant("Hi."),
            Message::user("Again"),
        ],
        max_output_tokens: 256,
        temperature: Some(0.3),
    }
}

fn body_of(raw: &str) -> serde_json::Value {
    serde_json::from_str(&raw[raw.find("\r\n\r\n").unwrap() + 4..]).unwrap()
}

const STREAM: &str = "event: message_start\n\
data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"type\":\"message\",\"usage\":{\"input_tokens\":7,\"cache_read_input_tokens\":3,\"output_tokens\":1}}}\n\n\
event: content_block_start\n\
data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"\"}}\n\n\
event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"private\"}}\n\n\
event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"signature_delta\",\"signature\":\"sig\"}}\n\n\
event: ping\n\
data: {\"type\":\"ping\"}\n\n\
event: content_block_start\n\
data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n\
event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"text_delta\",\"text\":\"Bonjour \"}}\n\n\
event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"text_delta\",\"text\":\"é漢\"}}\n\n\
event: content_block_stop\n\
data: {\"type\":\"content_block_stop\",\"index\":1}\n\n\
event: message_delta\n\
data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":12}}\n\n\
event: message_stop\n\
data: {\"type\":\"message_stop\"}\n\n";

#[tokio::test]
async fn streams_text_only_and_sends_the_messages_shape() {
    let bytes = STREAM.as_bytes();
    let split = bytes.windows(3).position(|w| w == "漢".as_bytes()).unwrap() + 1;
    let (base, seen) = serve(Reply::sse(&[&bytes[..split], &bytes[split..]])).await;
    let (tx, rx) = mpsc::channel(4);
    let adapter = adapter(&base, Some("sk-ant-secret")).await;
    let req = request();
    let (completion, events) = tokio::join!(adapter.stream(&req, tx), drain(rx));
    let completion = completion.unwrap();
    assert_eq!(completion.text, "Bonjour é漢");
    assert_eq!(completion.finish, FinishReason::Stop);
    let usage = Usage {
        input_tokens: Some(10),
        output_tokens: Some(12),
    };
    assert_eq!(completion.usage, Some(usage));
    assert_eq!(
        events,
        vec![
            StreamEvent::TextDelta {
                text: "Bonjour ".into()
            },
            StreamEvent::TextDelta {
                text: "é漢".into()
            },
            StreamEvent::Usage { usage },
            StreamEvent::Finished {
                reason: FinishReason::Stop
            },
        ]
    );

    let raw = seen.await.unwrap();
    let head = raw[..raw.find("\r\n\r\n").unwrap()].to_ascii_lowercase();
    assert!(head.starts_with("post /v1/messages "));
    assert!(head.contains("x-api-key: sk-ant-secret"));
    assert!(head.contains(&format!("anthropic-version: {API_VERSION}")));
    assert!(!head.contains("authorization:"));
    let body = body_of(&raw);
    assert_eq!(body["system"], "Be brief.");
    assert_eq!(body["stream"], true);
    assert_eq!(body["max_tokens"], 256 + THINKING_HEADROOM);
    assert!(body.get("temperature").is_none(), "sampling is never sent");
    assert_eq!(body["messages"].as_array().unwrap().len(), 3);
}

#[tokio::test]
async fn a_stream_without_message_stop_is_interrupted() {
    let cut = &STREAM[..STREAM.find("event: message_delta").unwrap()];
    let (base, _seen) = serve(Reply::sse(&[cut.as_bytes()])).await;
    let (tx, rx) = mpsc::channel(16);
    let adapter = adapter(&base, None).await;
    let req = request();
    let (result, _) = tokio::join!(adapter.stream(&req, tx), drain(rx));
    assert_eq!(result.unwrap_err(), Error::Interrupted);
}

#[tokio::test]
async fn an_error_event_and_a_refusal_are_reported() {
    let error = "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":1}}}\n\n\
                 event: error\ndata: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"secret detail\"}}\n\n";
    let (base, _seen) = serve(Reply::sse(&[error.as_bytes()])).await;
    let (tx, rx) = mpsc::channel(16);
    let adapter = adapter(&base, None).await;
    let req = request();
    let (result, _) = tokio::join!(adapter.stream(&req, tx), drain(rx));
    let err = result.unwrap_err();
    assert_eq!(err, Error::Provider);
    assert!(!err.to_string().contains("secret"));

    let refused = STREAM.replace("end_turn", "refusal");
    let (base, _seen) = serve(Reply::sse(&[refused.as_bytes()])).await;
    let (tx, rx) = mpsc::channel(16);
    let adapter = self::adapter(&base, None).await;
    let req = request();
    let (result, _) = tokio::join!(adapter.stream(&req, tx), drain(rx));
    assert_eq!(result.unwrap().finish, FinishReason::ContentFilter);
}

#[tokio::test]
async fn non_streamed_answers_join_text_blocks_and_skip_thinking() {
    let json = r#"{"type":"message","content":[{"type":"thinking","thinking":"","signature":"s"},{"type":"text","text":"One "},{"type":"text","text":"two"}],"stop_reason":"max_tokens","usage":{"input_tokens":5,"cache_creation_input_tokens":20,"output_tokens":9}}"#;
    let (base, _seen) = serve(Reply::json("200 OK", "", json)).await;
    let completion = adapter(&base, None)
        .await
        .complete(&request())
        .await
        .unwrap();
    assert_eq!(completion.text, "One two");
    assert_eq!(completion.finish, FinishReason::Length);
    assert_eq!(
        completion.usage,
        Some(Usage {
            input_tokens: Some(25),
            output_tokens: Some(9)
        })
    );

    let only_thinking = r#"{"type":"message","content":[{"type":"thinking","thinking":""}],"stop_reason":"end_turn"}"#;
    let (base, _seen) = serve(Reply::json("200 OK", "", only_thinking)).await;
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
async fn status_errors_drop_the_body_and_a_prefill_is_refused_before_sending() {
    let (base, _seen) = serve(Reply::json(
        "529 Overloaded",
        "retry-after: 7\r\n",
        r#"{"type":"error","error":{"type":"overloaded_error","message":"echo sk-ant-secret"}}"#,
    ))
    .await;
    let err = adapter(&base, Some("sk-ant-secret"))
        .await
        .complete(&request())
        .await
        .unwrap_err();
    assert_eq!(
        err,
        Error::Http {
            status: 529,
            retry_after: Some(7)
        }
    );
    assert!(!err.to_string().contains("secret"));
    assert!(!err.may_have_been_accepted());

    // Nothing listens here: a request that reached the network would fail
    // as Unreachable, not Invalid.
    let mut prefill = request();
    prefill.messages.push(Message::assistant("Sure,"));
    let err = adapter("http://127.0.0.1:9/v1", None)
        .await
        .complete(&prefill)
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Invalid(_)), "{err:?}");
}

#[tokio::test]
async fn model_discovery_sends_the_version_and_lists_ids() {
    let json = r#"{"data":[{"id":"claude-sonnet-5-5","display_name":"Claude Sonnet 5.5"},{"id":"claude-opus-5-5"}],"has_more":false}"#;
    let (base, seen) = serve(Reply::json("200 OK", "", json)).await;
    let ids = adapter(&base, Some("k")).await.list_models().await.unwrap();
    assert_eq!(ids, vec!["claude-opus-5-5", "claude-sonnet-5-5"]);
    let raw = seen.await.unwrap().to_ascii_lowercase();
    assert!(raw.starts_with("get /v1/models?limit=1000 "));
    assert!(raw.contains("anthropic-version:"));
    assert!(raw.contains("x-api-key: k"));
}

#[tokio::test]
async fn debug_output_redacts_the_key() {
    let printed = format!(
        "{:?}",
        adapter("http://127.0.0.1:9/v1", Some("sk-ant-x")).await
    );
    assert!(!printed.contains("sk-ant-x"));
    assert!(printed.contains("has_key: true"));
}
