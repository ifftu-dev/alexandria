use std::time::Duration;

use serde_json::json;
use url::Url;

use crate::model::{bounded, StudioConnection};
use crate::{Error, Result};

pub fn validate_connection(connection: &StudioConnection) -> Result<Url> {
    if connection.name.trim().is_empty() || connection.model.trim().is_empty() {
        return Err(Error::Invalid(
            "A connection needs a display name and model identifier".into(),
        ));
    }
    bounded(&connection.name, 200, "Connection name")?;
    bounded(&connection.model, 200, "Model identifier")?;
    let mut url = Url::parse(&connection.endpoint)
        .map_err(|_| Error::Invalid("Enter a valid API base URL".into()))?;
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(Error::Invalid(
            "API base URLs cannot contain credentials, query strings, or fragments".into(),
        ));
    }
    let local = match url.host() {
        Some(url::Host::Domain("localhost")) => true,
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        _ => false,
    };
    match connection.location.as_str() {
        "local" if local && matches!(url.scheme(), "http" | "https") => {}
        "cloud" if url.scheme() == "https" && !local => {}
        _ => {
            return Err(Error::Invalid(
                "Local endpoints must use loopback; cloud endpoints must use HTTPS".into(),
            ))
        }
    }
    if connection.capability != "text" {
        return Err(Error::Invalid(
            "Only OpenAI-compatible text endpoints are currently supported".into(),
        ));
    }
    let path = format!("{}/", url.path().trim_end_matches('/'));
    url.set_path(&path);
    Ok(url)
}

pub async fn complete(
    connection: &StudioConnection,
    key: Option<&str>,
    system: &str,
    context: &str,
) -> Result<String> {
    if !connection.enabled {
        return Err(Error::Unavailable("Model connection is disabled".into()));
    }
    let base = validate_connection(connection)?;
    if connection.location == "cloud" && key.is_none_or(str::is_empty) {
        return Err(Error::Invalid(
            "Add an API key for this cloud connection".into(),
        ));
    }
    let mut builder = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .connect_timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none());
    if connection.location == "local" {
        builder = builder.no_proxy();
    }
    let client = builder
        .build()
        .map_err(|_| Error::Unavailable("Could not initialize the model connection".into()))?;
    let url = base
        .join("chat/completions")
        .map_err(|_| Error::Invalid("Invalid completion endpoint".into()))?;
    let mut request=client.post(url).json(&json!({"model":connection.model,"messages":[{"role":"system","content":system},{"role":"user","content":context}],"stream":false,"max_tokens":4096}));
    if let Some(key) = key.filter(|s| !s.is_empty()) {
        request = request.bearer_auth(key);
    }
    let mut response = request.send().await.map_err(|e| {
        Error::Unavailable(
            if e.is_timeout() {
                "Model request timed out"
            } else {
                "Could not reach the selected model"
            }
            .into(),
        )
    })?;
    if !response.status().is_success() {
        return Err(Error::Unavailable(format!(
            "Model returned HTTP {}; check the connection and provider allowance",
            response.status().as_u16()
        )));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| Error::Unavailable("Model response interrupted".into()))?
    {
        if bytes.len() + chunk.len() > 256_000 {
            return Err(Error::Invalid(
                "Model response exceeds the size limit".into(),
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|_| Error::Unavailable("Model returned invalid JSON".into()))?;
    let text = value
        .pointer("/choices/0/message/content")
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| Error::Unavailable("The model returned no text".into()))?;
    bounded(text, 128_000, "Model output")?;
    Ok(text.into())
}
