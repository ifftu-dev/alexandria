//! The package scheme handler: one request in, one response out.
//!
//! - Only `GET` and `HEAD`, only under an open launch prefix, only files
//!   inside that launch's package directory (no parent steps, no links out).
//! - A `Range` request reads that range from disk and nothing else. A file
//!   above [`ServeLimits::whole_file`] is served only by range.
//! - HTML is always sent whole, with the bootstrap linked as its first
//!   element. Documents that cannot carry it (SVG, XHTML, XML) are sent
//!   whole with a `sandbox` policy, so their script never runs.
//! - In every document, a `<link>` that would preload with `as=fetch` loses
//!   its `as`: WebKit sends such a preload to any origin, whatever the
//!   content policy says.
//! - Every response, refusals included, carries the package content policy
//!   and a permissions policy that grants no device capability.

use std::io::SeekFrom;
use std::path::{Component, Path, PathBuf};

use tauri::http::header::{HeaderValue, CONTENT_TYPE, RANGE};
use tauri::http::{Method, Request, Response, StatusCode};
use tokio::io::{AsyncReadExt, AsyncSeekExt};

use super::launches::{Launches, OpenLaunch};
use super::RESERVED_SEGMENT;

/// How much the handler sends in one response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServeLimits {
    /// Largest file sent whole. A larger file is served only by range, and
    /// an HTML page above it is not served.
    pub whole_file: u64,
    /// Largest slice sent for one range request; a longer or open-ended
    /// range is answered with its first `range_chunk` bytes.
    pub range_chunk: u64,
}

impl ServeLimits {
    pub const DEFAULT: ServeLimits = ServeLimits {
        whole_file: 32 * 1024 * 1024,
        range_chunk: 8 * 1024 * 1024,
    };
}

/// The package content policy. Inline script and evaluation are allowed
/// because legacy authoring tools emit both; everything that loads or sends
/// stays on the package origin; plugins and objects are blocked.
const CONTENT_POLICY: &str = "default-src 'self'; \
    script-src 'self' 'unsafe-inline' 'unsafe-eval'; \
    style-src 'self' 'unsafe-inline'; \
    img-src 'self' data: blob:; \
    media-src 'self' data: blob:; \
    font-src 'self' data:; \
    connect-src 'self'; \
    frame-src 'self'; \
    child-src 'self'; \
    worker-src 'self'; \
    manifest-src 'none'; \
    object-src 'none'; \
    base-uri 'self'; \
    form-action 'self'";

/// Added for documents that cannot carry the bootstrap.
const SANDBOXED_POLICY_SUFFIX: &str = "; sandbox";

/// Added for pages: string sinks go through the bootstrap's default Trusted
/// Types policy, which is what stops a javascript: address from replacing a
/// page with a document the bootstrap never ran in. Only pages: a worker
/// script has no bootstrap to install the policy.
const DOCUMENT_POLICY_SUFFIX: &str = "; require-trusted-types-for 'script'";

/// No device capability is granted to package content.
const PERMISSIONS_POLICY: &str = "camera=(), microphone=(), geolocation=(), \
    clipboard-read=(), fullscreen=(), display-capture=(), payment=(), usb=(), \
    serial=(), hid=(), bluetooth=(), midi=(), publickey-credentials-get=()";

const BOOTSTRAP_JS: &[u8] = include_bytes!("bootstrap.js");
const CLEAR_HTML: &[u8] = include_bytes!("clear.html");
const SCORM12_JS: &[u8] = include_bytes!("../../resources/scorm-again/scorm12.min.js");
const SCORM2004_JS: &[u8] = include_bytes!("../../resources/scorm-again/scorm2004.min.js");
const WRAPPER_HTML: &[u8] = include_bytes!("wrapper.html");
const WRAPPER_JS: &[u8] = include_bytes!("wrapper.js");
const RETURNED_HTML: &[u8] = include_bytes!("returned.html");

/// The app's own files under every launch prefix, at
/// `<prefix>/_alexandria/<name>`. The wrapper is the player's page at the
/// package origin: it holds the run-time API for the lesson frame inside it.
const RESERVED_FILES: &[(&str, &[u8])] = &[
    ("bootstrap.js", BOOTSTRAP_JS),
    ("clear.html", CLEAR_HTML),
    ("wrapper.html", WRAPPER_HTML),
    ("wrapper.js", WRAPPER_JS),
    ("scorm12.min.js", SCORM12_JS),
    ("scorm2004.min.js", SCORM2004_JS),
    // A cmi5 unit's `returnURL`: tells the player the unit exited.
    ("returned.html", RETURNED_HTML),
];

/// Answers one request for package content.
pub async fn serve(
    launches: &Launches,
    limits: ServeLimits,
    request: Request<Vec<u8>>,
) -> Response<Vec<u8>> {
    let head_only = match *request.method() {
        Method::GET => false,
        Method::HEAD => true,
        _ => return refusal(StatusCode::METHOD_NOT_ALLOWED),
    };
    // Service worker scripts are fetched with this header; refusing them is
    // what makes registration fail even if the bootstrap is bypassed.
    if request.headers().contains_key("service-worker") {
        return refusal(StatusCode::FORBIDDEN);
    }

    let raw = request.uri().path().trim_start_matches('/');
    let Some((prefix, raw_path)) = raw.split_once('/') else {
        return refusal(StatusCode::NOT_FOUND);
    };
    let Some(launch) = launches.get(prefix) else {
        return refusal(StatusCode::NOT_FOUND);
    };
    let Some(path) = percent_decode(raw_path) else {
        return refusal(StatusCode::NOT_FOUND);
    };

    let mut response = if let Some(name) = reserved_name(&path) {
        serve_reserved(prefix, name)
    } else {
        let range = request
            .headers()
            .get(RANGE)
            .and_then(|value| value.to_str().ok());
        serve_file(&launch, prefix, &path, range, limits).await
    };
    if let Some(port) = launch.endpoint_port {
        admit_endpoint(&mut response, port);
    }
    if head_only {
        response.body_mut().clear();
    }
    response
}

/// Lets the content of a launch with an open cmi5 session connect to that
/// session's loopback endpoint: the literal address and port, nothing wider.
fn admit_endpoint(response: &mut Response<Vec<u8>>, port: u16) {
    let Some(policy) = response
        .headers()
        .get("Content-Security-Policy")
        .and_then(|value| value.to_str().ok())
    else {
        return;
    };
    let admitted = policy.replacen(
        "connect-src 'self';",
        &format!("connect-src 'self' http://127.0.0.1:{port};"),
        1,
    );
    if let Ok(value) = HeaderValue::from_str(&admitted) {
        response
            .headers_mut()
            .insert("Content-Security-Policy", value);
    }
}

/// The name under the reserved segment, if the path is in it.
fn reserved_name(path: &str) -> Option<&str> {
    if path == RESERVED_SEGMENT {
        return Some("");
    }
    path.strip_prefix(RESERVED_SEGMENT)?.strip_prefix('/')
}

fn serve_reserved(prefix: &str, name: &str) -> Response<Vec<u8>> {
    let Some((_, bytes)) = RESERVED_FILES.iter().find(|(file, _)| *file == name) else {
        return refusal(StatusCode::NOT_FOUND);
    };
    let kind = Kind::of(Path::new(name));
    let body = if kind == Kind::Html {
        inject_bootstrap(bytes, prefix)
    } else {
        bytes.to_vec()
    };
    let mut response = respond(StatusCode::OK, kind, body);
    if name == "clear.html" {
        response.headers_mut().insert(
            "Clear-Site-Data",
            HeaderValue::from_static("\"cache\", \"storage\""),
        );
    }
    response
}

async fn serve_file(
    launch: &OpenLaunch,
    prefix: &str,
    path: &str,
    range: Option<&str>,
    limits: ServeLimits,
) -> Response<Vec<u8>> {
    let Some(file) = resolve(&launch.root, path).await else {
        return refusal(StatusCode::NOT_FOUND);
    };
    let Ok(mut handle) = tokio::fs::File::open(&file).await else {
        return refusal(StatusCode::NOT_FOUND);
    };
    let Ok(size) = handle.metadata().await.map(|m| m.len()) else {
        return refusal(StatusCode::NOT_FOUND);
    };
    let kind = Kind::of(&file);

    // A document is always sent whole, because it is rewritten: a page gets
    // the bootstrap, and no document keeps a fetch preload.
    if matches!(kind, Kind::Html | Kind::Sandboxed(_)) {
        if size > limits.whole_file {
            log::warn!(
                "package document above the whole-file cap: {}",
                file.display()
            );
            return refusal(StatusCode::NOT_FOUND);
        }
        let Some(bytes) = read_slice(&mut handle, 0, size).await else {
            return refusal(StatusCode::NOT_FOUND);
        };
        let bytes = without_fetch_preloads(&bytes);
        let bytes = if kind == Kind::Html {
            inject_bootstrap(&bytes, prefix)
        } else {
            bytes
        };
        return respond(StatusCode::OK, kind, bytes);
    }

    let Some(range) = range else {
        if size > limits.whole_file {
            return unsatisfiable(kind, size);
        }
        return match read_slice(&mut handle, 0, size).await {
            Some(bytes) => {
                let mut response = respond(StatusCode::OK, kind, bytes);
                set(&mut response, "Accept-Ranges", "bytes");
                response
            }
            None => refusal(StatusCode::NOT_FOUND),
        };
    };

    let Some((start, end)) = parse_range(range, size, limits.range_chunk) else {
        return unsatisfiable(kind, size);
    };
    match read_slice(&mut handle, start, end - start + 1).await {
        Some(bytes) => {
            let mut response = respond(StatusCode::PARTIAL_CONTENT, kind, bytes);
            set(&mut response, "Accept-Ranges", "bytes");
            set(
                &mut response,
                "Content-Range",
                &format!("bytes {start}-{end}/{size}"),
            );
            response
        }
        None => refusal(StatusCode::NOT_FOUND),
    }
}

/// The file at `path` inside `root`, if it is a regular file there. Refuses
/// absolute paths, parent steps, backslashes and anything whose real path,
/// links followed, leaves `root`.
async fn resolve(root: &Path, path: &str) -> Option<PathBuf> {
    if path.is_empty() || path.contains('\\') || path.contains('\0') {
        return None;
    }
    let relative = Path::new(path);
    if !relative
        .components()
        .all(|component| matches!(component, Component::Normal(_)))
    {
        return None;
    }
    let real = tokio::fs::canonicalize(root.join(relative)).await.ok()?;
    if !real.starts_with(root) {
        return None;
    }
    let metadata = tokio::fs::metadata(&real).await.ok()?;
    metadata.is_file().then_some(real)
}

/// Reads `len` bytes from `start`, and only those.
async fn read_slice(file: &mut tokio::fs::File, start: u64, len: u64) -> Option<Vec<u8>> {
    file.seek(SeekFrom::Start(start)).await.ok()?;
    let mut bytes = Vec::with_capacity(usize::try_from(len).ok()?);
    file.take(len).read_to_end(&mut bytes).await.ok()?;
    (bytes.len() as u64 == len).then_some(bytes)
}

/// The first range of a `bytes=` header as inclusive offsets, shortened to
/// `chunk` bytes. `None` when it cannot be satisfied.
fn parse_range(header: &str, size: u64, chunk: u64) -> Option<(u64, u64)> {
    let spec = header.trim().strip_prefix("bytes=")?;
    let first = spec.split(',').next()?.trim();
    let (start, end) = first.split_once('-')?;
    let (start, end) = match (start.trim(), end.trim()) {
        ("", suffix) => {
            let suffix: u64 = suffix.parse().ok()?;
            if suffix == 0 || size == 0 {
                return None;
            }
            (size.saturating_sub(suffix), size - 1)
        }
        (start, "") => (start.parse().ok()?, size.checked_sub(1)?),
        (start, end) => {
            let start: u64 = start.parse().ok()?;
            let end: u64 = end.parse().ok()?;
            (start, end.min(size.checked_sub(1)?))
        }
    };
    if start > end || start >= size || chunk == 0 {
        return None;
    }
    Some((start, end.min(start + chunk - 1)))
}

/// What a file is, for its content type and how it is sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Html,
    /// A document type that can run script but cannot carry the bootstrap.
    Sandboxed(&'static str),
    Other(&'static str),
}

impl Kind {
    fn of(path: &Path) -> Kind {
        let extension = path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase);
        match extension.as_deref() {
            Some("html" | "htm") => Kind::Html,
            Some("svg" | "svgz") => Kind::Sandboxed("image/svg+xml"),
            Some("xhtml" | "xht") => Kind::Sandboxed("application/xhtml+xml"),
            Some("xml" | "xsd" | "xsl" | "xslt") => Kind::Sandboxed("application/xml"),
            Some(other) => Kind::Other(content_type(other)),
            None => Kind::Other("application/octet-stream"),
        }
    }

    fn content_type(self) -> &'static str {
        match self {
            // No charset: a legacy page declares its own.
            Kind::Html => "text/html",
            Kind::Sandboxed(t) | Kind::Other(t) => t,
        }
    }
}

fn content_type(extension: &str) -> &'static str {
    match extension {
        "js" | "mjs" => "text/javascript",
        "css" => "text/css",
        "json" => "application/json",
        "txt" => "text/plain",
        "csv" => "text/csv",
        "vtt" => "text/vtt",
        "wasm" => "application/wasm",
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "bmp" => "image/bmp",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "eot" => "application/vnd.ms-fontobject",
        "mp3" => "audio/mpeg",
        "m4a" => "audio/mp4",
        "aac" => "audio/aac",
        "wav" => "audio/wav",
        "ogg" | "oga" => "audio/ogg",
        "ogv" => "video/ogg",
        "mp4" | "m4v" => "video/mp4",
        "webm" => "video/webm",
        "mov" => "video/quicktime",
        _ => "application/octet-stream",
    }
}

/// Links the bootstrap as the first element of a page: after the opening
/// `<head>` tag, else after `<html>`, else after the doctype, else at the
/// very start. Works on bytes, so a page in any encoding is left intact.
fn inject_bootstrap(page: &[u8], prefix: &str) -> Vec<u8> {
    let tag = format!("<script src=\"/{prefix}/{RESERVED_SEGMENT}/bootstrap.js\"></script>");
    let at = after_open_tag(page, b"<head")
        .or_else(|| after_open_tag(page, b"<html"))
        .or_else(|| after_open_tag(page, b"<!doctype"))
        .unwrap_or(0);
    let mut out = Vec::with_capacity(page.len() + tag.len());
    out.extend_from_slice(&page[..at]);
    out.extend_from_slice(tag.as_bytes());
    out.extend_from_slice(&page[at..]);
    out
}

/// Renames the `as` attribute of every `<link>` tag whose `as` is `fetch`,
/// so the tag preloads nothing. Byte-level, like [`inject_bootstrap`].
fn without_fetch_preloads(page: &[u8]) -> Vec<u8> {
    const RENAMED: &[u8] = b"data-alexandria-as";
    let mut out = Vec::with_capacity(page.len());
    let mut from = 0;
    while let Some(start) = find_tag(page, from, b"<link") {
        let Some(close) = page[start..].iter().position(|&b| b == b'>') else {
            break;
        };
        let end = start + close;
        out.extend_from_slice(&page[from..start]);
        let tag = &page[start..end];
        let mut copied = 0;
        for at in fetch_as_attributes(tag) {
            out.extend_from_slice(&tag[copied..at]);
            out.extend_from_slice(RENAMED);
            copied = at + 2;
        }
        out.extend_from_slice(&tag[copied..]);
        from = end;
    }
    out.extend_from_slice(&page[from..]);
    out
}

/// Where `<name` starts at or after `from`, the name ending there.
fn find_tag(page: &[u8], from: usize, name: &[u8]) -> Option<usize> {
    let mut at = from;
    while let Some(found) = page
        .get(at..)?
        .windows(name.len())
        .position(|w| w.eq_ignore_ascii_case(name))
    {
        let start = at + found;
        if matches!(
            page.get(start + name.len()).copied(),
            Some(b'>' | b' ' | b'\t' | b'\n' | b'\r' | b'/' | b'\x0c')
        ) {
            return Some(start);
        }
        at = start + 1;
    }
    None
}

/// Offsets, within one tag, of each `as` attribute name whose value is
/// `fetch` (any case, quoted or not). Text inside other attribute values is
/// skipped.
fn fetch_as_attributes(tag: &[u8]) -> Vec<usize> {
    let mut found = Vec::new();
    let mut i = tag
        .iter()
        .position(|b| b.is_ascii_whitespace())
        .unwrap_or(tag.len());
    while i < tag.len() {
        while i < tag.len() && (tag[i].is_ascii_whitespace() || tag[i] == b'/') {
            i += 1;
        }
        let name_start = i;
        while i < tag.len() && !tag[i].is_ascii_whitespace() && !matches!(tag[i], b'=' | b'/') {
            i += 1;
        }
        let name = &tag[name_start..i];
        while i < tag.len() && tag[i].is_ascii_whitespace() {
            i += 1;
        }
        if tag.get(i) != Some(&b'=') {
            if name.is_empty() {
                i += 1;
            }
            continue;
        }
        i += 1;
        while i < tag.len() && tag[i].is_ascii_whitespace() {
            i += 1;
        }
        let (value_start, value_end) = match tag.get(i) {
            Some(&quote @ (b'"' | b'\'')) => {
                let start = i + 1;
                let end = tag[start..]
                    .iter()
                    .position(|&b| b == quote)
                    .map_or(tag.len(), |p| start + p);
                i = (end + 1).min(tag.len());
                (start, end)
            }
            _ => {
                let start = i;
                while i < tag.len() && !tag[i].is_ascii_whitespace() {
                    i += 1;
                }
                (start, i)
            }
        };
        if name.eq_ignore_ascii_case(b"as")
            && tag[value_start..value_end]
                .trim_ascii()
                .eq_ignore_ascii_case(b"fetch")
        {
            found.push(name_start);
        }
    }
    found
}

/// The offset just past the first `<name …>` tag, matched without case and
/// only when the name ends there (`<head` does not match `<header>`).
fn after_open_tag(page: &[u8], name: &[u8]) -> Option<usize> {
    let mut from = 0;
    while let Some(found) = page[from..]
        .windows(name.len())
        .position(|w| w.eq_ignore_ascii_case(name))
    {
        let start = from + found;
        let next = page.get(start + name.len()).copied();
        if matches!(next, Some(b'>' | b' ' | b'\t' | b'\n' | b'\r' | b'/')) {
            let close = page[start..].iter().position(|&b| b == b'>')?;
            return Some(start + close + 1);
        }
        from = start + 1;
    }
    None
}

/// Headers every response carries.
fn respond(status: StatusCode, kind: Kind, body: Vec<u8>) -> Response<Vec<u8>> {
    let policy = match kind {
        Kind::Sandboxed(_) => format!("{CONTENT_POLICY}{SANDBOXED_POLICY_SUFFIX}"),
        Kind::Html => format!("{CONTENT_POLICY}{DOCUMENT_POLICY_SUFFIX}"),
        Kind::Other(_) => CONTENT_POLICY.to_string(),
    };
    Response::builder()
        .status(status)
        .header(CONTENT_TYPE, kind.content_type())
        .header("Content-Security-Policy", policy)
        .header("Permissions-Policy", PERMISSIONS_POLICY)
        .header("X-Content-Type-Options", "nosniff")
        .header("Referrer-Policy", "no-referrer")
        .header("Cache-Control", "no-store")
        .header("Cross-Origin-Resource-Policy", "same-origin")
        .header("X-DNS-Prefetch-Control", "off")
        .body(body)
        .expect("package response headers are static and valid")
}

fn set(response: &mut Response<Vec<u8>>, name: &'static str, value: &str) {
    if let Ok(value) = HeaderValue::from_str(value) {
        response.headers_mut().insert(name, value);
    }
}

/// An empty refusal. Says nothing about whether a launch or file exists.
fn refusal(status: StatusCode) -> Response<Vec<u8>> {
    respond(status, Kind::Other("text/plain"), Vec::new())
}

fn unsatisfiable(kind: Kind, size: u64) -> Response<Vec<u8>> {
    let mut response = respond(StatusCode::RANGE_NOT_SATISFIABLE, kind, Vec::new());
    set(&mut response, "Accept-Ranges", "bytes");
    set(&mut response, "Content-Range", &format!("bytes */{size}"));
    response
}

/// Decodes `%XX` escapes; `None` for a broken escape or invalid UTF-8.
fn percent_decode(input: &str) -> Option<String> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let pair = bytes.get(i + 1..i + 3)?;
            if !pair.iter().all(u8::is_ascii_hexdigit) {
                return None;
            }
            let hex = std::str::from_utf8(pair).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}
