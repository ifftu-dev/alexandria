//! What AI vendors allow third-party apps to do, as enforced here.
//!
//! Alexandria only ever calls a provider's developer API with the user's own
//! API key, billed to the key's owner. Vendors forbid third-party apps from
//! using consumer-plan sign-ins, so this module refuses them outright rather
//! than let a user's subscription account be suspended:
//!
//! - Anthropic: "developers may not collect, store, or intermediate Claude.ai
//!   credentials or session tokens"; Free, Pro, and Max sign-in tokens are
//!   for Anthropic's own apps (code.claude.com, Legal and compliance).
//! - OpenAI: ChatGPT sign-in for third-party apps is a gated partner
//!   program; ChatGPT and Codex session tokens are not API keys.
//! - Google and GitHub: the Gemini app, the Gemini CLI sign-in backend, and
//!   Copilot are subscription products, not developer APIs.
//!
//! Some developer APIs also set an age floor. Google's Gemini API requires
//! users to be 18 or older and forbids use in apps likely to be accessed by
//! people under 18 (Gemini API Additional Terms, effective 2026-03-23).
//! OpenRouter requires users to be 18 or older. The host refuses these for
//! minors; [`adults_only`] names them.
//!
//! The lists are deliberately small and conservative: they name hosts and
//! credential shapes, not every vendor. Review them when vendor terms change.

use url::{Host, Url};

use crate::{Error, Result};

/// Consumer-app backends. Requests to them would ride on a subscription
/// sign-in, which vendors do not allow third-party apps to use.
const CONSUMER_BACKENDS: &[(&str, &str)] = &[
    ("chatgpt.com", "ChatGPT"),
    ("chat.openai.com", "ChatGPT"),
    ("claude.ai", "Claude.ai"),
    ("gemini.google.com", "the Gemini app"),
    (
        "cloudcode-pa.googleapis.com",
        "the Gemini CLI sign-in service",
    ),
    ("githubcopilot.com", "GitHub Copilot"),
];

/// Developer APIs whose terms require every user to be 18 or older.
const ADULTS_ONLY: &[(&str, &str)] = &[
    ("generativelanguage.googleapis.com", "Google's Gemini API"),
    ("openrouter.ai", "OpenRouter"),
];

/// Developer APIs whose terms also restrict apps that minors are likely to
/// use, so adults are told before they connect one.
const RESTRICTS_APPS_FOR_MINORS: &[&str] = &["generativelanguage.googleapis.com"];

fn domain(url: &Url) -> Option<String> {
    match url.host()? {
        Host::Domain(d) => Some(d.trim_end_matches('.').to_ascii_lowercase()),
        _ => None,
    }
}

fn matches(domain: &str, suffix: &str) -> bool {
    domain == suffix || domain.ends_with(&format!(".{suffix}"))
}

fn lookup(url: &Url, table: &[(&str, &'static str)]) -> Option<&'static str> {
    let domain = domain(url)?;
    table
        .iter()
        .find(|(suffix, _)| matches(&domain, suffix))
        .map(|(_, name)| *name)
}

/// Refuses consumer-app backends. Checked when an endpoint is parsed, so
/// such a connection cannot be saved or used.
pub fn check_endpoint(url: &Url) -> Result<()> {
    match lookup(url, CONSUMER_BACKENDS) {
        Some(name) => Err(Error::Invalid(format!(
            "{name} is a consumer app, not a developer API. Alexandria can only use a provider's API with your own API key."
        ))),
        None => Ok(()),
    }
}

/// Refuses consumer sign-in tokens pasted where an API key belongs. Checked
/// before any request is sent, whatever the endpoint.
pub fn check_credential(url: &Url, key: &str) -> Result<()> {
    let key = key.trim();
    if key.starts_with("sk-ant-oat") || key.starts_with("sk-ant-ort") {
        return Err(Error::Invalid(
            "That is a Claude subscription sign-in token, which Anthropic does not allow in other apps. Create an API key in the Claude Console instead.".into(),
        ));
    }
    let on_openai = domain(url).is_some_and(|d| matches(&d, "openai.com"));
    if on_openai && looks_like_jwt(key) {
        return Err(Error::Invalid(
            "That is a ChatGPT sign-in token, not an API key. Create an API key in the OpenAI platform instead.".into(),
        ));
    }
    Ok(())
}

fn looks_like_jwt(key: &str) -> bool {
    let parts: Vec<&str> = key.split('.').collect();
    key.starts_with("eyJ")
        && parts.len() == 3
        && parts.iter().all(|p| {
            !p.is_empty()
                && p.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        })
}

/// The vendor's name when its terms require every user to be 18 or older.
pub fn adults_only(url: &Url) -> Option<&'static str> {
    lookup(url, ADULTS_ONLY)
}

/// Whether the vendor's terms also restrict use inside apps that minors are
/// likely to use, which adults should know before connecting.
pub fn restricts_apps_for_minors(url: &Url) -> bool {
    domain(url).is_some_and(|d| RESTRICTS_APPS_FOR_MINORS.iter().any(|s| matches(&d, s)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(raw: &str) -> Url {
        Url::parse(raw).unwrap()
    }

    #[test]
    fn consumer_backends_are_refused_and_developer_apis_are_not() {
        for raw in [
            "https://chatgpt.com/backend-api/codex",
            "https://chat.openai.com/v1",
            "https://claude.ai/api",
            "https://cloudcode-pa.googleapis.com/v1internal",
            "https://api.githubcopilot.com",
            "https://CHATGPT.COM./x",
        ] {
            assert!(check_endpoint(&url(raw)).is_err(), "{raw}");
        }
        for raw in [
            "https://api.openai.com/v1",
            "https://api.anthropic.com/v1",
            "https://generativelanguage.googleapis.com/v1beta/openai",
            "https://notchatgpt.com/v1",
            "https://openrouter.ai/api/v1",
        ] {
            assert!(check_endpoint(&url(raw)).is_ok(), "{raw}");
        }
    }

    #[test]
    fn subscription_tokens_are_refused_and_api_keys_are_not() {
        let anthropic = url("https://api.anthropic.com/v1");
        let openai = url("https://api.openai.com/v1");
        assert!(check_credential(&anthropic, "sk-ant-oat01-abc").is_err());
        assert!(check_credential(&openai, " sk-ant-ort01-abc").is_err());
        assert!(check_credential(&anthropic, "sk-ant-api03-abc").is_ok());
        let jwt = "eyJhbGciOiJSUzI1NiJ9.eyJzdWIiOiJ1In0.c2lnbmF0dXJl";
        assert!(check_credential(&openai, jwt).is_err());
        assert!(check_credential(&openai, "sk-proj-abc").is_ok());
        // Other vendors may use signed tokens as keys.
        assert!(check_credential(&url("https://example.com/v1"), jwt).is_ok());
    }

    #[test]
    fn age_floors_are_named() {
        let gemini = url("https://generativelanguage.googleapis.com/v1beta/openai/");
        assert_eq!(adults_only(&gemini), Some("Google's Gemini API"));
        assert!(restricts_apps_for_minors(&gemini));
        assert_eq!(
            adults_only(&url("https://openrouter.ai/api/v1")),
            Some("OpenRouter")
        );
        assert!(!restricts_apps_for_minors(&url(
            "https://openrouter.ai/api/v1"
        )));
        assert_eq!(adults_only(&url("https://api.openai.com/v1")), None);
    }
}
