//! Handler-seam tests: a request goes in as the webview would send it, and
//! the response the package frame would receive comes out. No webview.

use std::path::Path;

use tauri::http::{Request, Response, StatusCode};
use tempfile::TempDir;

use super::*;

const HASH_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const HASH_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

/// A package's extracted files on disk.
fn files(entries: &[(&str, &[u8])]) -> TempDir {
    let root = TempDir::new().expect("temp dir");
    for (path, bytes) in entries {
        let path = root.path().join(path);
        std::fs::create_dir_all(path.parent().unwrap()).expect("dirs");
        std::fs::write(path, bytes).expect("write file");
    }
    root
}

/// The URL a frame at the launch would request, in the form every platform
/// hands the handler.
fn url(prefix: &str, path: &str) -> String {
    format!("package://localhost/{prefix}/{path}")
}

fn get(url: &str) -> Request<Vec<u8>> {
    Request::builder()
        .method("GET")
        .uri(url)
        .body(Vec::new())
        .unwrap()
}

fn ranged(url: &str, range: &str) -> Request<Vec<u8>> {
    Request::builder()
        .method("GET")
        .uri(url)
        .header("Range", range)
        .body(Vec::new())
        .unwrap()
}

async fn send(launches: &Launches, request: Request<Vec<u8>>) -> Response<Vec<u8>> {
    serve(launches, ServeLimits::DEFAULT, request).await
}

fn header<'a>(response: &'a Response<Vec<u8>>, name: &str) -> Option<&'a str> {
    response
        .headers()
        .get(name)
        .map(|value| value.to_str().expect("ascii header"))
}

fn body(response: &Response<Vec<u8>>) -> String {
    String::from_utf8(response.body().clone()).expect("utf-8 body")
}

/// 0..=255 repeated, so any slice is recognisable.
fn media(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i % 251) as u8).collect()
}

fn open(launches: &Launches, hash: &str, root: &Path) -> String {
    launches.open(hash, root).expect("open launch").prefix
}

#[tokio::test]
async fn a_ranged_request_returns_only_that_range() {
    let video = media(10_000);
    let package = files(&[("media/intro.mp4", &video)]);
    let launches = Launches::new();
    let prefix = open(&launches, HASH_A, package.path());

    let response = send(
        &launches,
        ranged(&url(&prefix, "media/intro.mp4"), "bytes=1000-1999"),
    )
    .await;

    assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(response.body(), &video[1000..2000]);
    assert_eq!(
        header(&response, "Content-Range"),
        Some("bytes 1000-1999/10000")
    );
    assert_eq!(header(&response, "Accept-Ranges"), Some("bytes"));
    assert_eq!(header(&response, "Content-Type"), Some("video/mp4"));
}

#[tokio::test]
async fn open_ended_and_suffix_ranges_are_answered_within_the_chunk_limit() {
    let video = media(10_000);
    let package = files(&[("intro.mp4", &video)]);
    let launches = Launches::new();
    let prefix = open(&launches, HASH_A, package.path());
    let limits = ServeLimits {
        whole_file: 1_000,
        range_chunk: 4_096,
    };

    let start = serve(
        &launches,
        limits,
        ranged(&url(&prefix, "intro.mp4"), "bytes=0-"),
    )
    .await;
    assert_eq!(start.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(start.body(), &video[..4_096]);
    assert_eq!(header(&start, "Content-Range"), Some("bytes 0-4095/10000"));

    let tail = serve(
        &launches,
        limits,
        ranged(&url(&prefix, "intro.mp4"), "bytes=-500"),
    )
    .await;
    assert_eq!(tail.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(tail.body(), &video[9_500..]);
    assert_eq!(
        header(&tail, "Content-Range"),
        Some("bytes 9500-9999/10000")
    );
}

#[tokio::test]
async fn a_range_past_the_end_is_refused_with_the_file_size() {
    let package = files(&[("intro.mp4", &media(100))]);
    let launches = Launches::new();
    let prefix = open(&launches, HASH_A, package.path());

    let response = send(
        &launches,
        ranged(&url(&prefix, "intro.mp4"), "bytes=100-200"),
    )
    .await;

    assert_eq!(response.status(), StatusCode::RANGE_NOT_SATISFIABLE);
    assert_eq!(header(&response, "Content-Range"), Some("bytes */100"));
    assert!(response.body().is_empty());
}

#[tokio::test]
async fn a_file_above_the_whole_file_cap_is_served_only_by_range() {
    let video = media(5_000);
    let package = files(&[("intro.mp4", &video), ("small.png", b"png")]);
    let launches = Launches::new();
    let prefix = open(&launches, HASH_A, package.path());
    let limits = ServeLimits {
        whole_file: 1_000,
        range_chunk: 4_096,
    };

    let whole = serve(&launches, limits, get(&url(&prefix, "intro.mp4"))).await;
    assert_eq!(whole.status(), StatusCode::RANGE_NOT_SATISFIABLE);
    assert_eq!(header(&whole, "Content-Range"), Some("bytes */5000"));
    assert!(whole.body().is_empty());

    let small = serve(&launches, limits, get(&url(&prefix, "small.png"))).await;
    assert_eq!(small.status(), StatusCode::OK);
    assert_eq!(small.body(), b"png");
}

#[tokio::test]
async fn a_closed_launch_prefix_is_refused() {
    let package = files(&[("index.html", b"<html><body>hi</body></html>")]);
    let launches = Launches::new();
    let prefix = open(&launches, HASH_A, package.path());
    assert_eq!(
        send(&launches, get(&url(&prefix, "index.html")))
            .await
            .status(),
        StatusCode::OK
    );

    assert!(launches.close(&prefix));

    let response = send(&launches, get(&url(&prefix, "index.html"))).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert!(!body(&response).contains("hi"));
    assert!(!launches.close(&prefix));
}

#[tokio::test]
async fn a_prefix_that_was_never_opened_is_refused() {
    let package = files(&[("index.html", b"<html></html>")]);
    let launches = Launches::new();
    open(&launches, HASH_A, package.path());

    for path in [
        "0123456789abcdef0123456789abcdef/index.html",
        "index.html",
        "",
        &format!("{HASH_A}/index.html"),
    ] {
        let response = send(&launches, get(&format!("package://localhost/{path}"))).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
    }
}

#[tokio::test]
async fn one_launch_cannot_reach_another_packages_files() {
    let cache = TempDir::new().unwrap();
    for (hash, text) in [(HASH_A, "package a"), (HASH_B, "package b secret")] {
        let dir = cache.path().join(hash);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("index.html"), text).unwrap();
    }
    std::fs::write(cache.path().join("outside.txt"), "outside").unwrap();
    let launches = Launches::new();
    let a = open(&launches, HASH_A, &cache.path().join(HASH_A));

    for path in [
        format!("../{HASH_B}/index.html"),
        format!("..%2F{HASH_B}%2Findex.html"),
        format!("%2E%2E/{HASH_B}/index.html"),
        "../outside.txt".to_string(),
        "/etc/hosts".to_string(),
        "%2Fetc%2Fhosts".to_string(),
        "..\\outside.txt".to_string(),
    ] {
        let response = send(&launches, get(&url(&a, &path))).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
        assert!(!body(&response).contains("secret"), "{path}");
        assert!(!body(&response).contains("outside"), "{path}");
    }
}

#[cfg(unix)]
#[tokio::test]
async fn a_link_out_of_the_package_is_not_followed() {
    let package = files(&[("index.html", b"<html></html>")]);
    let secret = files(&[("secret.txt", b"secret")]);
    std::os::unix::fs::symlink(
        secret.path().join("secret.txt"),
        package.path().join("link.txt"),
    )
    .unwrap();
    let launches = Launches::new();
    let prefix = open(&launches, HASH_A, package.path());

    let response = send(&launches, get(&url(&prefix, "link.txt"))).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert!(!body(&response).contains("secret"));
}

#[tokio::test]
async fn closing_every_launch_refuses_all_their_prefixes() {
    let package = files(&[("index.html", b"<html></html>")]);
    let launches = Launches::new();
    let first = open(&launches, HASH_A, package.path());
    let second = open(&launches, HASH_A, package.path());
    assert_ne!(first, second);
    assert!(launches.is_package_open(HASH_A));

    launches.close_all();

    assert!(!launches.is_package_open(HASH_A));
    for prefix in [first, second] {
        let response = send(&launches, get(&url(&prefix, "index.html"))).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}

// ── Content policy, bootstrap and the reserved path ─────────────────────

fn policy(response: &Response<Vec<u8>>) -> String {
    header(response, "Content-Security-Policy")
        .expect("every response carries the package content policy")
        .to_string()
}

fn directive(policy: &str, name: &str) -> String {
    policy
        .split(';')
        .map(str::trim)
        .find(|d| d.split_whitespace().next() == Some(name))
        .unwrap_or_else(|| panic!("policy has no {name}: {policy}"))
        .to_string()
}

#[tokio::test]
async fn every_response_carries_the_package_content_policy() {
    let package = files(&[
        ("index.html", b"<html><head></head><body></body></html>"),
        ("app.js", b"1"),
        ("intro.mp4", &media(100)),
    ]);
    let launches = Launches::new();
    let prefix = open(&launches, HASH_A, package.path());

    let responses = [
        send(&launches, get(&url(&prefix, "index.html"))).await,
        send(&launches, get(&url(&prefix, "app.js"))).await,
        send(&launches, ranged(&url(&prefix, "intro.mp4"), "bytes=0-9")).await,
        send(&launches, get(&url(&prefix, "missing.html"))).await,
        send(
            &launches,
            get(&url("0123456789abcdef0123456789abcdef", "index.html")),
        )
        .await,
        send(&launches, get(&url(&prefix, "_alexandria/bootstrap.js"))).await,
    ];

    for response in &responses {
        let policy = policy(response);
        // Legacy authoring tools emit inline script and evaluate strings.
        let script = directive(&policy, "script-src");
        assert!(script.contains("'self'"), "{policy}");
        assert!(script.contains("'unsafe-inline'"), "{policy}");
        assert!(script.contains("'unsafe-eval'"), "{policy}");
        // Requests, frames, media, fonts, images and forms stay on the
        // package origin; plugins and objects are blocked.
        for name in [
            "default-src",
            "connect-src",
            "frame-src",
            "media-src",
            "font-src",
            "img-src",
            "form-action",
        ] {
            let d = directive(&policy, name);
            assert!(d.contains("'self'"), "{name}: {policy}");
            assert!(!d.contains("http"), "{name}: {policy}");
            assert!(!d.contains('*'), "{name}: {policy}");
        }
        assert_eq!(directive(&policy, "object-src"), "object-src 'none'");
        // No device capabilities.
        let permissions = header(response, "Permissions-Policy").expect("permissions policy");
        for feature in [
            "camera=()",
            "microphone=()",
            "geolocation=()",
            "clipboard-read=()",
            "fullscreen=()",
        ] {
            assert!(permissions.contains(feature), "{permissions}");
        }
        assert_eq!(header(response, "X-Content-Type-Options"), Some("nosniff"));
        assert_eq!(header(response, "Cache-Control"), Some("no-store"));
        assert_eq!(header(response, "Referrer-Policy"), Some("no-referrer"));
    }
}

#[tokio::test]
async fn every_html_page_loads_the_bootstrap_before_its_own_script() {
    let launches = Launches::new();
    let package = files(&[
        (
            "index.html",
            b"<!DOCTYPE html><html><head><script>window.mine = 1</script></head><body></body></html>",
        ),
        ("bare.htm", b"<SCRIPT>window.mine = 1</SCRIPT><p>no head</p>"),
        (
            "doctype.html",
            b"<!doctype html><header>not the head</header><script>1</script>",
        ),
        (
            "HEAD.HTML",
            b"<HTML><HEAD lang=\"en\"><script>1</script></HEAD></HTML>",
        ),
    ]);
    let prefix = open(&launches, HASH_A, package.path());
    let tag = format!("<script src=\"/{prefix}/_alexandria/bootstrap.js\"></script>");

    for path in ["index.html", "bare.htm", "doctype.html", "HEAD.HTML"] {
        let response = send(&launches, get(&url(&prefix, path))).await;
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        let html = body(&response);
        let bootstrap = html.find(&tag).unwrap_or_else(|| panic!("{path}: {html}"));
        let first_script = html
            .to_ascii_lowercase()
            .match_indices("<script")
            .map(|(i, _)| i)
            .find(|&i| i != bootstrap)
            .expect("the page's own script");
        assert!(bootstrap < first_script, "{path}: {html}");
        assert_eq!(
            header(&response, "Content-Type"),
            Some("text/html"),
            "{path}"
        );
    }

    // A doctype stays first, so the page keeps its rendering mode.
    let doctype = body(&send(&launches, get(&url(&prefix, "doctype.html"))).await);
    assert!(doctype.starts_with("<!doctype html>"), "{doctype}");
}

#[tokio::test]
async fn an_html_page_keeps_its_own_encoding() {
    // windows-1252 "café"; the page declares its own charset.
    let page: &[u8] =
        b"<html><head><meta charset=\"windows-1252\"></head><body>caf\xe9</body></html>";
    let package = files(&[("index.html", page)]);
    let launches = Launches::new();
    let prefix = open(&launches, HASH_A, package.path());

    let response = send(&launches, get(&url(&prefix, "index.html"))).await;

    assert!(response.body().ends_with(b"<body>caf\xe9</body></html>"));
    assert_eq!(header(&response, "Content-Type"), Some("text/html"));
}

#[tokio::test]
async fn a_ranged_request_for_a_page_still_gets_the_whole_page_with_the_bootstrap() {
    let package = files(&[(
        "index.html",
        b"<html><head></head><body>lesson</body></html>",
    )]);
    let launches = Launches::new();
    let prefix = open(&launches, HASH_A, package.path());

    let response = send(&launches, ranged(&url(&prefix, "index.html"), "bytes=0-5")).await;

    assert_eq!(response.status(), StatusCode::OK);
    let html = body(&response);
    assert!(html.contains("_alexandria/bootstrap.js"), "{html}");
    assert!(html.contains("lesson"), "{html}");
}

#[tokio::test]
async fn the_reserved_path_serves_the_apps_files_and_never_the_packages() {
    let package = files(&[
        ("_alexandria/bootstrap.js", b"window.hostile = true"),
        ("_alexandria/scorm12.min.js", b"window.hostile = true"),
        ("_alexandria/other.js", b"window.hostile = true"),
    ]);
    let launches = Launches::new();
    let prefix = open(&launches, HASH_A, package.path());

    let bootstrap = send(&launches, get(&url(&prefix, "_alexandria/bootstrap.js"))).await;
    assert_eq!(bootstrap.status(), StatusCode::OK);
    assert_eq!(header(&bootstrap, "Content-Type"), Some("text/javascript"));
    assert!(!body(&bootstrap).contains("hostile"));
    assert!(body(&bootstrap).contains("RTCPeerConnection"));

    for (name, global) in [
        ("scorm12.min.js", "Scorm12API"),
        ("scorm2004.min.js", "Scorm2004API"),
    ] {
        let runtime = send(
            &launches,
            get(&url(&prefix, &format!("_alexandria/{name}"))),
        )
        .await;
        assert_eq!(runtime.status(), StatusCode::OK, "{name}");
        assert_eq!(header(&runtime, "Content-Type"), Some("text/javascript"));
        assert!(!body(&runtime).contains("hostile"), "{name}");
        assert!(body(&runtime).contains(global), "{name}");
    }

    for path in [
        "_alexandria/other.js",
        "%5Falexandria/other.js",
        "_alexandria/",
    ] {
        let response = send(&launches, get(&url(&prefix, path))).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
        assert!(!body(&response).contains("hostile"), "{path}");
    }
}

#[tokio::test]
async fn the_storage_clearing_page_asks_the_engine_to_clear_the_origin() {
    let package = files(&[("index.html", b"<html></html>")]);
    let launches = Launches::new();
    let prefix = open(&launches, HASH_A, package.path());

    let response = send(&launches, get(&url(&prefix, "_alexandria/clear.html"))).await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        header(&response, "Clear-Site-Data"),
        Some("\"cache\", \"storage\"")
    );
    let html = body(&response);
    assert!(html.contains("_alexandria/bootstrap.js"), "{html}");
    assert!(html.contains("storage-cleared"), "{html}");
}

#[tokio::test]
async fn the_player_wrapper_is_served_from_the_reserved_path_with_the_bootstrap_first() {
    let package = files(&[
        ("index.html", b"<html></html>"),
        ("_alexandria/wrapper.html", b"<html>hostile</html>"),
        ("_alexandria/wrapper.js", b"window.hostile = true"),
    ]);
    let launches = Launches::new();
    let prefix = open(&launches, HASH_A, package.path());

    let page = send(&launches, get(&url(&prefix, "_alexandria/wrapper.html"))).await;
    assert_eq!(page.status(), StatusCode::OK);
    assert_eq!(header(&page, "Content-Type"), Some("text/html"));
    let html = body(&page);
    assert!(!html.contains("hostile"), "{html}");
    let bootstrap = html.find("_alexandria/bootstrap.js").expect("bootstrap");
    let wrapper = html.find("wrapper.js").expect("wrapper script");
    assert!(bootstrap < wrapper, "the bootstrap runs before the wrapper");

    let script = send(&launches, get(&url(&prefix, "_alexandria/wrapper.js"))).await;
    assert_eq!(script.status(), StatusCode::OK);
    assert_eq!(header(&script, "Content-Type"), Some("text/javascript"));
    assert!(!body(&script).contains("hostile"));
    assert!(body(&script).contains("alexandria-wrapper"));
}

#[tokio::test]
async fn a_service_worker_script_is_never_served() {
    let package = files(&[("sw.js", b"self.addEventListener('fetch', () => {})")]);
    let launches = Launches::new();
    let prefix = open(&launches, HASH_A, package.path());

    let request = Request::builder()
        .method("GET")
        .uri(url(&prefix, "sw.js"))
        .header("Service-Worker", "script")
        .body(Vec::new())
        .unwrap();
    let response = send(&launches, request).await;

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(response.body().is_empty());
    assert!(header(&response, "Content-Security-Policy").is_some());
}

#[tokio::test]
async fn documents_that_cannot_carry_the_bootstrap_are_sandboxed() {
    let package = files(&[
        (
            "logo.svg",
            b"<svg xmlns=\"http://www.w3.org/2000/svg\"><script>1</script></svg>",
        ),
        (
            "page.xhtml",
            b"<html xmlns=\"http://www.w3.org/1999/xhtml\"></html>",
        ),
        ("data.xml", b"<data/>"),
    ]);
    let launches = Launches::new();
    let prefix = open(&launches, HASH_A, package.path());

    for path in ["logo.svg", "page.xhtml", "data.xml"] {
        let response = send(&launches, get(&url(&prefix, path))).await;
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        assert_eq!(
            directive(&policy(&response), "sandbox"),
            "sandbox",
            "{path}"
        );
    }
}

#[tokio::test]
async fn only_reads_are_answered() {
    let package = files(&[("index.html", b"<html></html>")]);
    let launches = Launches::new();
    let prefix = open(&launches, HASH_A, package.path());

    let post = Request::builder()
        .method("POST")
        .uri(url(&prefix, "index.html"))
        .body(b"x".to_vec())
        .unwrap();
    assert_eq!(
        send(&launches, post).await.status(),
        StatusCode::METHOD_NOT_ALLOWED
    );

    let head = Request::builder()
        .method("HEAD")
        .uri(url(&prefix, "index.html"))
        .body(Vec::new())
        .unwrap();
    let response = send(&launches, head).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.body().is_empty());
}

// ── Opening a launch of a stored package ────────────────────────────────

mod fixture {
    //! A stored-only zip writer, copied from the packages module's tests.

    fn crc32(data: &[u8]) -> u32 {
        let mut crc = !0u32;
        for &byte in data {
            crc ^= u32::from(byte);
            for _ in 0..8 {
                crc = if crc & 1 == 1 {
                    (crc >> 1) ^ 0xEDB8_8320
                } else {
                    crc >> 1
                };
            }
        }
        !crc
    }

    pub fn zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut directory = Vec::new();
        for (name, data) in entries {
            let offset = out.len() as u32;
            let mut shared = Vec::new();
            shared.extend_from_slice(&20u16.to_le_bytes());
            shared.extend_from_slice(&0x0800u16.to_le_bytes());
            shared.extend_from_slice(&0u16.to_le_bytes());
            shared.extend_from_slice(&0u16.to_le_bytes());
            shared.extend_from_slice(&0x21u16.to_le_bytes());
            shared.extend_from_slice(&crc32(data).to_le_bytes());
            shared.extend_from_slice(&(data.len() as u32).to_le_bytes());
            shared.extend_from_slice(&(data.len() as u32).to_le_bytes());
            shared.extend_from_slice(&(name.len() as u16).to_le_bytes());
            shared.extend_from_slice(&0u16.to_le_bytes());

            out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
            out.extend_from_slice(&shared);
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(data);

            directory.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
            directory.extend_from_slice(&((3u16 << 8) | 20).to_le_bytes());
            directory.extend_from_slice(&shared);
            directory.extend_from_slice(&0u16.to_le_bytes());
            directory.extend_from_slice(&0u16.to_le_bytes());
            directory.extend_from_slice(&0u16.to_le_bytes());
            directory.extend_from_slice(&(0o100644u32 << 16).to_le_bytes());
            directory.extend_from_slice(&offset.to_le_bytes());
            directory.extend_from_slice(name.as_bytes());
        }
        let directory_offset = out.len() as u32;
        out.extend_from_slice(&directory);
        out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(directory.len() as u32).to_le_bytes());
        out.extend_from_slice(&directory_offset.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }
}

const MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<manifest identifier="com.example.lesson" version="1.0"
    xmlns="http://www.imsproject.org/xsd/imscp_rootv1p1p2"
    xmlns:adlcp="http://www.adlnet.org/xsd/adlcp_rootv1p2">
  <metadata>
    <schema>ADL SCORM</schema>
    <schemaversion>1.2</schemaversion>
  </metadata>
  <organizations default="org">
    <organization identifier="org">
      <title>Lesson</title>
      <item identifier="welcome" identifierref="res-welcome">
        <title>Welcome</title>
      </item>
    </organization>
  </organizations>
  <resources>
    <resource identifier="res-welcome" type="webcontent" adlcp:scormtype="sco" href="welcome/index.html">
      <file href="welcome/index.html"/>
    </resource>
  </resources>
</manifest>"#;

/// A profile with one imported package.
struct Profile {
    db: std::sync::Mutex<crate::db::Database>,
    dirs: PackageDirs,
    node: ContentNode,
    hash: String,
    _root: TempDir,
}

impl Profile {
    async fn with_package() -> Self {
        let root = TempDir::new().expect("temp dir");
        let dirs = PackageDirs {
            archives: root.path().join("packages"),
            cache: root.path().join("packagecache"),
        };
        dirs.ensure().expect("package dirs");
        let node = ContentNode::new(&root.path().join("iroh"));
        node.start(None).await.expect("start node");
        let db = crate::db::Database::open_in_memory().expect("in-memory db");
        db.run_migrations().expect("migrations");
        let db = std::sync::Mutex::new(db);
        let archive = root.path().join("lesson.zip");
        std::fs::write(
            &archive,
            fixture::zip(&[
                ("imsmanifest.xml", MANIFEST.as_bytes()),
                (
                    "welcome/index.html",
                    b"<html><head></head><body>Welcome</body></html>",
                ),
            ]),
        )
        .expect("write archive");
        let package = packages::import(&db, &dirs, &archive)
            .await
            .expect("import");
        Self {
            db,
            dirs,
            node,
            hash: package.hash,
            _root: root,
        }
    }
}

type Reports = std::sync::Arc<std::sync::Mutex<Vec<PackageExtractProgress>>>;

fn recorder() -> (Reports, impl FnMut(PackageExtractProgress) + Send + 'static) {
    let reports = Reports::default();
    let sink = reports.clone();
    (reports, move |report| sink.lock().unwrap().push(report))
}

#[tokio::test]
async fn an_open_launch_serves_its_package_until_it_is_closed() {
    let profile = Profile::with_package().await;
    let launches = Launches::new();
    let (_, progress) = recorder();

    let launch = open_launch(
        &launches,
        &profile.db,
        &profile.node,
        &profile.dirs,
        &profile.hash.to_ascii_uppercase(),
        progress,
    )
    .await
    .expect("open launch");

    assert_eq!(launch.package_hash, profile.hash);
    assert_eq!(launch.prefix.len(), 32);
    assert!(launch.prefix.bytes().all(|b| b.is_ascii_hexdigit()));
    assert_eq!(launch.base_url, format!("{}/{}/", origin(), launch.prefix));
    assert!(launches.is_package_open(&profile.hash));

    let lesson = send(&launches, get(&url(&launch.prefix, "welcome/index.html"))).await;
    assert_eq!(lesson.status(), StatusCode::OK);
    assert!(body(&lesson).contains("Welcome"));
    assert!(body(&lesson).contains("_alexandria/bootstrap.js"));

    assert!(launches.close(&launch.prefix));
    assert!(!launches.is_package_open(&profile.hash));
    let after = send(&launches, get(&url(&launch.prefix, "welcome/index.html"))).await;
    assert_eq!(after.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn two_launches_opened_at_once_extract_the_package_once() {
    let profile = Profile::with_package().await;
    let launches = Launches::new();
    let (first_reports, first_progress) = recorder();
    let (second_reports, second_progress) = recorder();

    let (first, second) = tokio::join!(
        open_launch(
            &launches,
            &profile.db,
            &profile.node,
            &profile.dirs,
            &profile.hash,
            first_progress,
        ),
        open_launch(
            &launches,
            &profile.db,
            &profile.node,
            &profile.dirs,
            &profile.hash,
            second_progress,
        ),
    );
    let (first, second) = (first.expect("first"), second.expect("second"));

    assert_ne!(first.prefix, second.prefix);
    let extractions = [first_reports, second_reports]
        .iter()
        .filter(|reports| !reports.lock().unwrap().is_empty())
        .count();
    assert_eq!(extractions, 1);
}

#[tokio::test]
async fn a_launch_of_an_unknown_package_opens_nothing() {
    let profile = Profile::with_package().await;
    let launches = Launches::new();
    let (_, progress) = recorder();

    let refused = open_launch(
        &launches,
        &profile.db,
        &profile.node,
        &profile.dirs,
        HASH_B,
        progress,
    )
    .await;

    assert!(matches!(refused, Err(PackageError::NotFound(_))));
    assert!(!launches.is_package_open(HASH_B));
}
