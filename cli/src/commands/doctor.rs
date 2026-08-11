//! Environment diagnostics — project layout, app data, toolchain, and the
//! mobile build prerequisites needed by `alex run ios` / `alex run android`.
//!
//! This absorbs what used to live in three places: the prerequisite checks
//! from `alex build`, the app-data status from `alex health`, and the
//! project/tool listing from `alex config show`.

use anyhow::{Context, Result};
use clap::Args;
use owo_colors::OwoColorize;
use serde::Serialize;
use serde_json::json;
use std::fs;

use crate::context::ProjectContext;
use crate::output;
use crate::output::is_json;
use crate::runner;

/// Rust triples needed to run on a physical Android device, an Android
/// emulator, a physical iPhone, and the iOS simulator respectively.
const ANDROID_TRIPLES: &[&str] = &[
    "aarch64-linux-android",
    "armv7-linux-androideabi",
    "x86_64-linux-android",
];
const IOS_TRIPLES: &[&str] = &["aarch64-apple-ios", "aarch64-apple-ios-sim"];

#[derive(Args)]
pub struct DoctorArgs {
    /// Skip the mobile prerequisite checks (Android SDK/NDK/Java, Xcode,
    /// cross-compilation targets). Useful on a desktop-only machine.
    #[arg(long)]
    no_mobile: bool,
}

// ── Check result ─────────────────────────────────────────────────────

#[derive(Serialize)]
struct Check {
    label: String,
    ok: bool,
    detail: String,
}

impl Check {
    fn new(label: impl Into<String>, ok: bool, detail: impl Into<String>) -> Self {
        Check {
            label: label.into(),
            ok,
            detail: detail.into(),
        }
    }
}

fn display(checks: &[Check]) -> bool {
    if is_json() {
        // Sections are collected into the report document instead.
        return checks.iter().all(|c| c.ok);
    }
    for check in checks {
        let dot = if check.ok {
            "●".green().to_string()
        } else {
            "●".red().to_string()
        };
        let detail = if check.ok {
            check.detail.dimmed().to_string()
        } else {
            check.detail.red().to_string()
        };
        eprintln!("    {} {:30} {}", dot, check.label, detail);
    }
    checks.iter().all(|c| c.ok)
}

// ── Individual checks ────────────────────────────────────────────────

fn check_rust_target_installed(triple: &str) -> bool {
    runner::run_silent(
        &std::env::current_dir().unwrap_or_default(),
        "rustup",
        &["target", "list", "--installed"],
    )
    .map(|out| out.lines().any(|l| l.trim() == triple))
    .unwrap_or(false)
}

fn rust_target_checks(triples: &[&str]) -> Vec<Check> {
    triples
        .iter()
        .map(|triple| {
            let installed = check_rust_target_installed(triple);
            Check::new(
                format!("Rust target {}", triple),
                installed,
                if installed {
                    "installed".to_string()
                } else {
                    format!("run: rustup target add {}", triple)
                },
            )
        })
        .collect()
}

fn check_android_sdk() -> Check {
    match std::env::var("ANDROID_HOME").or_else(|_| std::env::var("ANDROID_SDK_ROOT")) {
        Ok(path) => {
            let exists = std::path::Path::new(&path).exists();
            Check::new(
                "Android SDK",
                exists,
                if exists {
                    path
                } else {
                    format!("{} (path does not exist)", path)
                },
            )
        }
        Err(_) => Check::new("Android SDK", false, "ANDROID_HOME not set"),
    }
}

fn check_android_ndk() -> Check {
    let home = std::env::var("ANDROID_HOME").unwrap_or_default();
    let ndk_dir = std::path::Path::new(&home).join("ndk");
    if !ndk_dir.exists() {
        return Check::new("Android NDK", false, "not installed");
    }

    let version = fs::read_dir(&ndk_dir).ok().and_then(|entries| {
        entries
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
            .filter_map(|e| e.file_name().into_string().ok())
            .max()
    });

    match version {
        Some(v) => Check::new("Android NDK", true, v),
        None => Check::new("Android NDK", false, "ndk/ exists but no version found"),
    }
}

fn check_java_version() -> Check {
    // `java -version` writes to stderr, so `run_silent` (which captures
    // stdout) comes back empty. Shell out directly and read stderr.
    match std::process::Command::new("java").arg("-version").output() {
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let line = stderr.lines().next().unwrap_or("").trim().to_string();
            Check::new(
                "Java",
                out.status.success(),
                if line.is_empty() {
                    "installed (version unknown)".to_string()
                } else {
                    line
                },
            )
        }
        Err(_) => Check::new("Java", false, "not found"),
    }
}

fn check_xcode() -> Check {
    match runner::run_silent(
        &std::env::current_dir().unwrap_or_default(),
        "xcodebuild",
        &["-version"],
    ) {
        Ok(out) => Check::new(
            "Xcode",
            true,
            out.lines().next().unwrap_or("installed").to_string(),
        ),
        Err(_) => Check::new("Xcode", false, "not installed (xcodebuild not found)"),
    }
}

fn check_tauri_cli(ctx: &ProjectContext) -> Check {
    match runner::run_silent(&ctx.root, "cargo", &["tauri", "--version"]) {
        Ok(out) => Check::new("Tauri CLI", true, out.trim().to_string()),
        Err(_) => Check::new(
            "Tauri CLI",
            false,
            "not installed (cargo install tauri-cli)",
        ),
    }
}

/// Report whether the NDK cross-compilation matrix resolves. `alex run
/// android` builds this env itself; surfacing it here turns a mid-build
/// linker failure into an up-front diagnostic.
fn check_android_env(ctx: &ProjectContext) -> Check {
    match crate::android_env::AndroidEnv::detect(&ctx.root) {
        Ok(env) => Check::new(
            "NDK toolchain",
            true,
            format!("API {} · {}", env.api_level, env.host_tag),
        ),
        Err(e) => Check::new("NDK toolchain", false, format!("{}", e)),
    }
}

// ── Sections ─────────────────────────────────────────────────────────

fn section_project(ctx: &ProjectContext) -> Result<()> {
    output::header("Project");
    output::kv("Root", &ctx.root.display().to_string());
    output::kv("Tauri dir", &ctx.tauri_dir.display().to_string());

    let conf_path = ctx.tauri_dir.join("tauri.conf.json");
    if conf_path.exists() {
        let conf_str = fs::read_to_string(&conf_path).context("Failed to read tauri.conf.json")?;
        if let Ok(conf) = serde_json::from_str::<serde_json::Value>(&conf_str) {
            for (key, label) in [
                ("productName", "Product"),
                ("identifier", "Identifier"),
                ("version", "Version"),
            ] {
                if let Some(value) = conf.get(key).and_then(|v| v.as_str()) {
                    output::kv(label, value);
                }
            }
        }
    }
    Ok(())
}

/// App data is reported as presence, not pass/fail — a missing database on a
/// machine where the app has never launched is normal, so rendering it as a
/// red failure (and dragging down the overall verdict) would be misleading.
fn section_app_data(ctx: &ProjectContext) {
    output::blank();
    output::header("App data");
    output::kv("Directory", &ctx.app_data_dir.display().to_string());

    for (label, present) in [
        ("Database", ctx.has_db()),
        ("Vault", ctx.has_vault()),
        ("Iroh store", ctx.iroh_dir().exists()),
    ] {
        output::kv(label, if present { "exists" } else { "not created" });
    }

    if !ctx.has_app_data() {
        output::faint("App has not been launched on this machine yet.");
    }
}

/// A named group of checks. Sections return their checks rather than only a
/// pass/fail so `--json` can report the same detail the human view shows.
struct Section {
    name: &'static str,
    checks: Vec<Check>,
}

impl Section {
    fn ok(&self) -> bool {
        self.checks.iter().all(|c| c.ok)
    }
}

fn section_toolchain(ctx: &ProjectContext) -> Section {
    output::blank();
    output::header("Toolchain");

    let mut checks: Vec<Check> = ["cargo", "rustc", "node", "npm"]
        .iter()
        .map(
            |tool| match runner::run_silent(&ctx.root, tool, &["--version"]) {
                Ok(v) => Check::new(*tool, true, v.trim().to_string()),
                Err(_) => Check::new(*tool, false, "not found"),
            },
        )
        .collect();
    checks.push(check_tauri_cli(ctx));
    display(&checks);
    Section {
        name: "toolchain",
        checks,
    }
}

fn section_mobile(ctx: &ProjectContext) -> Vec<Section> {
    output::blank();
    output::header("Android prerequisites");
    let mut android = vec![
        check_android_sdk(),
        check_android_ndk(),
        check_java_version(),
    ];
    android.push(check_android_env(ctx));
    android.extend(rust_target_checks(ANDROID_TRIPLES));
    display(&android);

    let mut sections = vec![Section {
        name: "android",
        checks: android,
    }];

    if !cfg!(target_os = "macos") {
        output::blank();
        output::faint("iOS checks skipped — they require macOS.");
        return sections;
    }

    output::blank();
    output::header("iOS prerequisites");
    let mut ios = vec![check_xcode()];
    ios.extend(rust_target_checks(IOS_TRIPLES));
    display(&ios);
    sections.push(Section {
        name: "ios",
        checks: ios,
    });

    sections
}

// ── Entry point ──────────────────────────────────────────────────────

pub fn execute(args: &DoctorArgs, ctx: &ProjectContext) -> Result<()> {
    section_project(ctx)?;
    section_app_data(ctx);

    let mut sections = vec![section_toolchain(ctx)];
    if !args.no_mobile {
        sections.extend(section_mobile(ctx));
    }
    let all_ok = sections.iter().all(Section::ok);

    output::blank();
    if all_ok {
        output::success("All checks passed");
    } else {
        output::warning("Some checks failed — see the red entries above");
    }
    output::blank();

    output::emit(&json!({
        "root": ctx.root.display().to_string(),
        "appDataDir": ctx.app_data_dir.display().to_string(),
        "appData": {
            "database": ctx.has_db(),
            "vault": ctx.has_vault(),
            "irohStore": ctx.iroh_dir().exists(),
        },
        "sections": sections
            .iter()
            .map(|s| json!({ "name": s.name, "ok": s.ok(), "checks": &s.checks }))
            .collect::<Vec<_>>(),
        "ok": all_ok,
    }))?;

    // Diagnostics always exit 0: a missing Android NDK is a fact to report,
    // not a failure of the `doctor` command itself. Callers branch on `ok`.
    Ok(())
}

/// Print the app data directory to stdout for scripting: `cd $(alex path)`.
///
/// The bare-path form is the whole point of this command, so human mode keeps
/// printing exactly the path and nothing else.
pub fn print_path(ctx: &ProjectContext) -> Result<()> {
    let path = ctx.app_data_dir.display().to_string();
    if is_json() {
        return output::emit(&json!({ "path": path }));
    }
    println!("{path}");
    Ok(())
}
