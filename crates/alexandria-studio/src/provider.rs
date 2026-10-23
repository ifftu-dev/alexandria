//! Studio's model calls, served by the shared `alexandria-ai` runtime.
//!
//! The public functions keep the shape Studio and the lesson tutor already
//! use. Endpoint policy, transport bounds, and error sanitizing live in
//! `alexandria-ai`, so Studio, the tutor, and global chat cannot drift apart.

use std::time::Duration;

use alexandria_ai::compatible::Compatible;
use alexandria_ai::endpoint::{self, Location};
use alexandria_ai::model::Limits;
use alexandria_ai::{ChatRequest, Message};
use url::Url;

use crate::model::{bounded, StudioConnection};
use crate::{Error, Result};

const LOCATION_ERROR: &str = "Local endpoints must use loopback; private-network endpoints must use a private address; cloud endpoints must use HTTPS";

/// The endpoint location a connection declares. `local` and `cloud` are the
/// Studio names; `private_network` is a server on the user's own network.
pub fn location(connection: &StudioConnection) -> Result<Location> {
    match connection.location.as_str() {
        "local" => Ok(Location::Device),
        "private_network" => Ok(Location::PrivateNetwork),
        "cloud" => Ok(Location::Remote),
        _ => Err(Error::Invalid(LOCATION_ERROR.into())),
    }
}

fn map(error: alexandria_ai::Error) -> Error {
    match error {
        alexandria_ai::Error::Invalid(message) => Error::Invalid(message),
        alexandria_ai::Error::TooLarge => {
            Error::Invalid("Model response exceeds the size limit".into())
        }
        other => Error::Unavailable(other.to_string()),
    }
}

pub fn validate_connection(connection: &StudioConnection) -> Result<Url> {
    if connection.name.trim().is_empty() || connection.model.trim().is_empty() {
        return Err(Error::Invalid(
            "A connection needs a display name and model identifier".into(),
        ));
    }
    bounded(&connection.name, 200, "Connection name")?;
    bounded(&connection.model, 200, "Model identifier")?;
    let location = location(connection)?;
    let url = endpoint::parse(&connection.endpoint, location).map_err(|error| match error {
        alexandria_ai::Error::Invalid(message) if message.starts_with("API base URLs") => {
            Error::Invalid(message)
        }
        alexandria_ai::Error::Invalid(message) if message.starts_with("Enter a valid") => {
            Error::Invalid(message)
        }
        _ => Error::Invalid(LOCATION_ERROR.into()),
    })?;
    if connection.capability != "text" {
        return Err(Error::Invalid(
            "Only OpenAI-compatible text endpoints are currently supported".into(),
        ));
    }
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
    validate_connection(connection)?;
    let location = location(connection)?;
    if location == Location::Remote && key.is_none_or(str::is_empty) {
        return Err(Error::Invalid(
            "Add an API key for this cloud connection".into(),
        ));
    }
    let endpoint = endpoint::resolve(&connection.endpoint, location)
        .await
        .map_err(map)?;
    let adapter = Compatible::new(endpoint, key.map(str::to_owned)).with_limits(Limits {
        total_timeout: Duration::from_secs(120),
        ..Limits::default()
    });
    let request = ChatRequest {
        model: connection.model.clone(),
        messages: vec![Message::system(system), Message::user(context)],
        max_output_tokens: 4096,
        temperature: None,
    };
    let completion = adapter.complete(&request).await.map_err(map)?;
    bounded(&completion.text, 128_000, "Model output")?;
    Ok(completion.text)
}
