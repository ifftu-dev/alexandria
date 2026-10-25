//! Studio's view of a model connection.
//!
//! A Studio connection document names its location in Studio's words
//! (`local`, `private_network`, `cloud`). This module maps it to what the
//! shared `alexandria-ai` runtime needs, validates a document before it is
//! saved, fixes the shape of a Studio run's request, and maps the runtime's
//! typed errors to Studio's vocabulary once. Reaching a model is
//! `alexandria_ai::connect`; endpoint policy, credential admission,
//! transport bounds, and error sanitizing live there, so Studio, the lesson
//! tutor, and global chat cannot drift apart.

use std::time::Duration;

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

/// The connection as `alexandria_ai::connect` takes it.
pub fn connection(connection: &StudioConnection) -> Result<alexandria_ai::Connection> {
    Ok(alexandria_ai::Connection {
        endpoint: connection.endpoint.clone(),
        location: location(connection)?,
        protocol: connection.protocol,
    })
}

/// Studio's reading of a runtime failure: a configuration problem is
/// invalid input; anything else is the model being unavailable, in the
/// runtime's own learner-safe words.
pub fn map_error(error: alexandria_ai::Error) -> Error {
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
        // A vendor-terms refusal explains itself.
        alexandria_ai::Error::Invalid(message) if message.contains("consumer app") => {
            Error::Invalid(message)
        }
        _ => Error::Invalid(LOCATION_ERROR.into()),
    })?;
    if (connection.protocol == alexandria_ai::Protocol::Responses)
        != (connection.auth == crate::model::ConnectionAuth::ChatgptPlan)
    {
        return Err(Error::Invalid(
            "The Responses protocol is only used with Sign in with ChatGPT".into(),
        ));
    }
    if connection.capability != "text" {
        return Err(Error::Invalid(
            "Only text connections are currently supported".into(),
        ));
    }
    Ok(url)
}

/// Bounds for one Studio run step or tutor turn: a single answer within two
/// minutes.
pub fn limits() -> Limits {
    Limits {
        total_timeout: Duration::from_secs(120),
        ..Limits::default()
    }
}

/// The request a Studio run step or tutor turn sends: the effective prompt
/// as instructions, the assembled context as the one user message.
pub fn request(connection: &StudioConnection, system: &str, context: &str) -> ChatRequest {
    ChatRequest {
        model: connection.model.clone(),
        messages: vec![Message::system(system), Message::user(context)],
        max_output_tokens: 4096,
        temperature: None,
    }
}
