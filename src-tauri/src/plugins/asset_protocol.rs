//! Custom `plugin://` URI scheme handler.
//!
//! A Tauri asset protocol that serves files from the installed plugin
//! directory. Each plugin gets its own origin (`plugin://<cid>/`), which
//! means same-origin policy gives us cross-plugin isolation for free.
//!
//! Responses carry a per-plugin Content-Security-Policy header:
//! - `default-src 'self' plugin://<cid>`  — only load from this plugin's origin
//! - `connect-src 'none'`                  — no network of any kind
//! - `script-src 'self' 'wasm-unsafe-eval'` — WASM for on-device inference,
//!   no inline scripts, no `eval`, no remote scripts
//!
//! HTML responses additionally have a small bootstrap script injected that
//! removes `window.__TAURI__` (defense in depth — the sandbox should already
//! block this) and exposes `window.alex` for the host↔plugin postMessage
//! protocol v1 (see `PluginIframe.vue`).

use std::path::Path;

use tauri::http::{Request, Response, StatusCode};

use crate::plugins::registry;

/// The bootstrap script injected at the top of every plugin HTML response.
/// Keep this minimal — it defines the v1 `window.alex` API and must not
/// leak any host capability beyond the postMessage channel.
const BOOTSTRAP_JS: &str = include_str!("bootstrap.js");

/// Per-plugin CSP. `{cid}` is replaced with the plugin's content address.
const PLUGIN_CSP_TEMPLATE: &str = "default-src 'self' plugin://{cid}; \
    connect-src 'none'; \
    img-src 'self' data: blob:; \
    media-src 'self' blob:; \
    style-src 'self' 'unsafe-inline'; \
    script-src 'self' 'wasm-unsafe-eval'; \
    font-src 'self' data:; \
    object-src 'none'; \
    base-uri 'none'; \
    form-action 'none'";

/// Synchronous handler for the `plugin://` scheme. Wired in via Tauri's
/// builder (`.register_uri_scheme_protocol`) and called on every request
/// the plugin iframe makes.
pub fn handle(plugins_dir: &Path, request: Request<Vec<u8>>) -> Response<Vec<u8>> {
    let uri = request.uri();
    let plugin_cid = match uri.host() {
        Some(h) => h.to_string(),
        None => return error_response(StatusCode::BAD_REQUEST, "missing plugin host"),
    };

    // Normalize the path: strip leading slash, default to entry file
    // when the URL is just `plugin://<cid>/`.
    let raw_path = uri.path().trim_start_matches('/');
    let asset_path = if raw_path.is_empty() {
        "ui/index.html".to_string()
    } else {
        raw_path.to_string()
    };

    // Decode percent-encoded path components. We accept only ASCII paths
    // in plugin bundles, but querystrings and URL-encoded spaces still
    // need to round-trip. Use a tiny inline decoder to avoid a new dep.
    let decoded_path = match percent_decode(&asset_path) {
        Some(p) => p,
        None => return error_response(StatusCode::BAD_REQUEST, "invalid plugin asset path"),
    };

    let resolved = match registry::resolve_asset(plugins_dir, &plugin_cid, &decoded_path) {
        Ok(p) => p,
        Err(e) => {
            log::warn!("plugin asset refused cid={plugin_cid} path={decoded_path}: {e}");
            return error_response(StatusCode::NOT_FOUND, "plugin asset not found");
        }
    };

    let bytes = match std::fs::read(&resolved) {
        Ok(b) => b,
        Err(e) => {
            log::warn!(
                "plugin asset read failed cid={plugin_cid} path={}: {e}",
                resolved.display()
            );
            return error_response(StatusCode::NOT_FOUND, "plugin asset read failed");
        }
    };

    let content_type = guess_content_type(&resolved);
    let is_html = content_type == "text/html; charset=utf-8";

    let body = if is_html {
        inject_bootstrap(&bytes)
    } else {
        bytes
    };

    let csp = PLUGIN_CSP_TEMPLATE.replace("{cid}", &plugin_cid);

    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", content_type)
        .header("Content-Security-Policy", csp)
        .header("X-Content-Type-Options", "nosniff")
        .header("Referrer-Policy", "no-referrer")
        // Prevent this response from being cached across plugin versions.
        // CID-addressed bundles can't collide, but a user re-installing the
        // same CID during development should still see fresh bytes.
        .header("Cache-Control", "no-store")
        .body(body)
        .unwrap_or_else(|_| {
            error_response(StatusCode::INTERNAL_SERVER_ERROR, "response build failed")
        })
}

fn error_response(status: StatusCode, msg: &'static str) -> Response<Vec<u8>> {
    Response::builder()
        .status(status)
        .header("Content-Type", "text/plain; charset=utf-8")
        .body(msg.as_bytes().to_vec())
        .expect("static error response must build")
}

fn guess_content_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("html" | "htm") => "text/html; charset=utf-8",
        Some("js" | "mjs") => "application/javascript; charset=utf-8",
        Some("wasm") => "application/wasm",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("ico") => "image/x-icon",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        Some("ttf") => "font/ttf",
        Some("otf") => "font/otf",
        Some("mp3") => "audio/mpeg",
        Some("wav") => "audio/wav",
        Some("ogg") => "audio/ogg",
        Some("mp4") => "video/mp4",
        Some("webm") => "video/webm",
        Some("txt") => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// Inject the bootstrap script at the earliest point inside the HTML so
/// plugin authors can rely on `window.alex` being defined before any of
/// their own scripts run.
fn inject_bootstrap(html_bytes: &[u8]) -> Vec<u8> {
    let html = String::from_utf8_lossy(html_bytes);
    let script_tag = format!("<script>/* alex:bootstrap */\n{}\n</script>", BOOTSTRAP_JS);

    let lower = html.to_ascii_lowercase();
    // Preferred injection point: right after `<head>` so the bootstrap
    // runs before any author scripts. Fall back to `<html>`, then to
    // prepending if neither is present (malformed HTML).
    let inject_after = if let Some(idx) = lower.find("<head>") {
        Some(idx + "<head>".len())
    } else if let Some(idx) = lower.find("<html>") {
        Some(idx + "<html>".len())
    } else if let Some(idx) = lower.find("<head ") {
        // `<head class="...">` — find the closing `>`.
        html[idx..].find('>').map(|end| idx + end + 1)
    } else {
        None
    };

    match inject_after {
        Some(pos) => {
            let mut out = String::with_capacity(html.len() + script_tag.len());
            out.push_str(&html[..pos]);
            out.push_str(&script_tag);
            out.push_str(&html[pos..]);
            out.into_bytes()
        }
        None => {
            let mut out = script_tag.into_bytes();
            out.extend_from_slice(html_bytes);
            out
        }
    }
}

/// Minimal percent-decoder: only handles `%XX` sequences, returns `None`
/// on invalid escapes. We accept `/` literals through unchanged.
fn percent_decode(input: &str) -> Option<String> {
    let mut out = Vec::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'%' {
            if i + 2 >= bytes.len() {
                return None;
            }
            let h = hex_digit(bytes[i + 1])?;
            let l = hex_digit(bytes[i + 2])?;
            out.push((h << 4) | l);
            i += 3;
        } else {
            out.push(b);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn injects_after_head() {
        let html = b"<html><head></head><body></body></html>";
        let out = inject_bootstrap(html);
        let s = String::from_utf8(out).unwrap();
        let head_idx = s.find("<head>").unwrap();
        let script_idx = s.find("<script>").unwrap();
        assert!(script_idx > head_idx);
        // Script must land before </head> closer.
        let close_idx = s.find("</head>").unwrap();
        assert!(script_idx < close_idx);
    }

    #[test]
    fn falls_back_to_prepending_on_malformed_html() {
        let html = b"no tags at all";
        let out = inject_bootstrap(html);
        let s = String::from_utf8(out).unwrap();
        assert!(s.starts_with("<script>"));
        assert!(s.ends_with("no tags at all"));
    }

    #[test]
    fn percent_decode_basic() {
        assert_eq!(percent_decode("hello").unwrap(), "hello");
        assert_eq!(percent_decode("hello%20world").unwrap(), "hello world");
        assert_eq!(percent_decode("a%2Fb").unwrap(), "a/b");
        assert!(percent_decode("bad%").is_none());
        assert!(percent_decode("bad%ZZ").is_none());
    }

    #[test]
    fn content_type_detection() {
        assert_eq!(
            guess_content_type(Path::new("a.html")),
            "text/html; charset=utf-8"
        );
        assert_eq!(
            guess_content_type(Path::new("a.HTML")),
            "text/html; charset=utf-8"
        );
        assert_eq!(
            guess_content_type(Path::new("a.js")),
            "application/javascript; charset=utf-8"
        );
        assert_eq!(guess_content_type(Path::new("a.wasm")), "application/wasm");
        assert_eq!(
            guess_content_type(Path::new("a.unknown")),
            "application/octet-stream"
        );
    }
}
