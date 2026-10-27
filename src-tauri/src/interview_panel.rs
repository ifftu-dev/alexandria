// SPDX-License-Identifier: AGPL-3.0-or-later
//! An interview panel's join link, and requests to Cloud on its behalf.
//!
//! The panel commands open and score the interview; the call manager keeps
//! the panelist's Cloud presence alive for the connected room. Both need the
//! link and the request, so they live here rather than in either.

use serde_json::Value;

use crate::cloud::{self, client, response_json};

/// What a join link carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelLink {
    pub cloud: String,
    pub interview_id: String,
    pub panelist_id: String,
    pub token: String,
}

/// Read `alexandria://interview/join?cloud=…&id=…&panelist=…&token=…`.
///
/// The Cloud origin comes from the link, so it is held to the same rule as
/// every other Cloud address: https, or http on loopback for local testing.
/// Anything else would send the panelist's token over plaintext.
pub fn parse_panel_link(link: &str) -> Result<PanelLink, String> {
    let url = reqwest::Url::parse(link.trim()).map_err(|_| "not an interview link".to_string())?;
    if url.scheme() != "alexandria"
        || url.host_str() != Some("interview")
        || url.path().trim_end_matches('/') != "/join"
    {
        return Err("not an interview link".into());
    }
    let param = |name: &str| {
        url.query_pairs()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .ok_or_else(|| format!("the interview link has no {name}"))
    };
    let cloud = param("cloud")?.trim_end_matches('/').to_string();
    if !cloud::is_allowed_origin(&cloud) {
        return Err("the interview link points at a service that is not https".into());
    }
    let interview_id = param("id")?;
    let panelist_id = param("panelist")?;
    for id in [&interview_id, &panelist_id] {
        uuid::Uuid::parse_str(id).map_err(|_| "the interview link is damaged".to_string())?;
    }
    let token = param("token")?;
    if token.len() < 32
        || token.len() > 128
        || !token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err("the interview link is damaged".into());
    }
    Ok(PanelLink {
        cloud,
        interview_id,
        panelist_id,
        token,
    })
}

/// Call Cloud's panel API for this link's interview, authenticated by the
/// panelist's token.
pub(crate) async fn panel_request(
    link: &PanelLink,
    method: reqwest::Method,
    suffix: &str,
    body: Option<&Value>,
) -> Result<Value, String> {
    let url = format!(
        "{}/api/panel/interviews/{}{suffix}",
        link.cloud, link.interview_id
    );
    let mut request = client()?
        .request(method, url)
        .header("authorization", format!("Panelist {}", link.token));
    if let Some(body) = body {
        request = request.json(body);
    }
    let response = request.send().await.map_err(|e| e.to_string())?;
    if response.status() == reqwest::StatusCode::NO_CONTENT {
        return Ok(Value::Null);
    }
    response_json(response).await
}
