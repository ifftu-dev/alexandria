//! Responses protocol contract tests against a real local HTTP server.

use alexandria_ai::endpoint::{self, Location};
use alexandria_ai::{
    Adapter, ChatRequest, Error, FinishReason, Message, Protocol, StreamEvent, Usage,
};
use tokio::sync::mpsc;

mod support;
use support::{drain, serve, Reply};

async fn adapter(base: &str, key: Option<&str>) -> Adapter {
    let endpoint = endpoint::resolve(base, Location::Device).await.unwrap();
    Adapter::new(Protocol::Responses, endpoint, key.map(str::to_owned))
}

fn request() -> ChatRequest {
    ChatRequest {
        model: "gpt-x".into(),
        messages: vec![Message::system("Be brief."), Message::user("Hello")],
        max_output_tokens: 256,
        temperature: Some(0.3),
    }
}

const STREAM: &str = "event: response.created\n\
data: {\"type\":\"response.created\",\"response\":{\"id\":\"r1\"}}\n\n\
event: response.output_text.delta\n\
data: {\"type\":\"response.output_text.delta\",\"delta\":\"Bonjour \"}\n\n\
event: response.reasoning_summary_text.delta\n\
data: {\"type\":\"response.reasoning_summary_text.delta\",\"delta\":\"private\"}\n\n\
event: response.output_text.delta\n\
data: {\"type\":\"response.output_text.delta\",\"delta\":\"é漢\"}\n\n\
event: response.completed\n\
data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"usage\":{\"input_tokens\":9,\"output_tokens\":4}}}\n\n";

#[tokio::test]
async fn streams_text_and_sends_the_plan_shape() {
    let token = "eyJhbGciOiJSUzI1NiJ9.eyJzdWIiOiJ1In0.c2ln";
    let (base, seen) = serve(Reply::sse(&[STREAM.as_bytes()])).await;
    let (tx, rx) = mpsc::channel(4);
    let adapter = adapter(&base, Some(token)).await;
    let req = request();
    let (completion, events) = tokio::join!(adapter.stream(&req, tx), drain(rx));
    let completion = completion.unwrap();
    assert_eq!(completion.text, "Bonjour é漢");
    assert_eq!(completion.finish, FinishReason::Stop);
    let usage = Usage {
        input_tokens: Some(9),
        output_tokens: Some(4),
    };
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
    assert!(head.starts_with("post /v1/responses "));
    assert!(head.contains(&format!(
        "authorization: bearer {}",
        token.to_ascii_lowercase()
    )));
    let body: serde_json::Value =
        serde_json::from_str(&raw[raw.find("\r\n\r\n").unwrap() + 4..]).unwrap();
    assert_eq!(body["instructions"], "Be brief.");
    assert_eq!(body["store"], false);
    assert_eq!(body["stream"], true);
    assert!(body.get("temperature").is_none());
    assert!(body.get("max_output_tokens").is_none());
}

#[tokio::test]
async fn a_stream_without_completion_is_interrupted_and_complete_collects() {
    let cut = &STREAM[..STREAM.find("event: response.completed").unwrap()];
    let (base, _seen) = serve(Reply::sse(&[cut.as_bytes()])).await;
    let (tx, rx) = mpsc::channel(16);
    let adapter = adapter(&base, None).await;
    let req = request();
    let (result, _) = tokio::join!(adapter.stream(&req, tx), drain(rx));
    assert_eq!(result.unwrap_err(), Error::Interrupted);

    let (base, _seen) = serve(Reply::sse(&[STREAM.as_bytes()])).await;
    let completion = self::adapter(&base, None)
        .await
        .complete(&request())
        .await
        .unwrap();
    assert_eq!(completion.text, "Bonjour é漢");
}

#[tokio::test]
async fn plan_limits_and_eligibility_are_recognised_without_showing_bodies() {
    let failed = "data: {\"type\":\"response.output_text.delta\",\"delta\":\"x\"}\n\n\
                  data: {\"type\":\"response.failed\",\"response\":{\"error\":{\"code\":\"subscription_sharing_usage_limit_exceeded\",\"message\":\"secret detail\"}}}\n\n";
    let (base, _seen) = serve(Reply::sse(&[failed.as_bytes()])).await;
    let (tx, rx) = mpsc::channel(16);
    let adapter = adapter(&base, None).await;
    let req = request();
    let (result, _) = tokio::join!(adapter.stream(&req, tx), drain(rx));
    let err = result.unwrap_err();
    assert_eq!(err, Error::UsageLimit);
    assert!(!err.to_string().contains("secret"));

    let (base, _seen) = serve(Reply::json(
        "403 Forbidden",
        "",
        r#"{"error":{"code":"subscription_sharing_user_not_eligible","message":"secret"}}"#,
    ))
    .await;
    let err = self::adapter(&base, None)
        .await
        .complete(&request())
        .await
        .unwrap_err();
    assert_eq!(err, Error::NotEligible);
    assert!(!err.may_have_been_accepted());

    let (base, _seen) = serve(Reply::json("401 Unauthorized", "", r#"{"detail":"x"}"#)).await;
    let err = self::adapter(&base, None)
        .await
        .complete(&request())
        .await
        .unwrap_err();
    assert!(err.is_auth(), "{err:?}");

    let incomplete = "data: {\"type\":\"response.output_text.delta\",\"delta\":\"part\"}\n\n\
                      data: {\"type\":\"response.incomplete\",\"response\":{\"incomplete_details\":{\"reason\":\"content_filter\"}}}\n\n";
    let (base, _seen) = serve(Reply::sse(&[incomplete.as_bytes()])).await;
    let done = self::adapter(&base, None)
        .await
        .complete(&request())
        .await
        .unwrap();
    assert_eq!(done.finish, FinishReason::ContentFilter);
}

#[tokio::test]
async fn model_discovery_keeps_listed_models_in_server_order() {
    let json = r#"{"models":[{"slug":"gpt-b","display_name":"B","visibility":"list"},{"slug":"gpt-hidden","visibility":"hide"},{"slug":"gpt-a","display_name":"A","visibility":"list"}]}"#;
    let (base, seen) = serve(Reply::json("200 OK", "", json)).await;
    let ids = adapter(&base, Some("t")).await.list_models().await.unwrap();
    assert_eq!(ids, vec!["gpt-b", "gpt-a"]);
    let raw = seen.await.unwrap().to_ascii_lowercase();
    assert!(raw.starts_with("get /v1/models "));
    assert!(raw.contains("authorization: bearer t"));
}
