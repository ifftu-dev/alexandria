use anyhow::{bail, Result};
use clap::Subcommand;
use console::style;
use dialoguer::{theme::ColorfulTheme, Confirm, MultiSelect, Select};
use owo_colors::OwoColorize;
use std::fmt;

use crate::context::ProjectContext;
use crate::output;
use crate::runner;

// ── Platform / target definitions ────────────────────────────────────

/// A build platform category.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Platform {
    Desktop,
    Android,
    Ios,
}

impl fmt::Display for Platform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Platform::Desktop => write!(f, "Desktop"),
            Platform::Android => write!(f, "Android"),
            Platform::Ios => write!(f, "iOS"),
        }
    }
}

/// A specific build target within a platform.
#[derive(Debug, Clone)]
struct Target {
    label: &'static str,
    platform: Platform,
    /// Rust triple (for display / prerequisite checks)
    rust_target: &'static str,
    /// Arguments passed to `cargo tauri build` or `cargo tauri <platform> build`
    build_args: &'static [&'static str],
}

/// All supported build targets.
fn all_targets() -> Vec<Target> {
    vec![
        // Desktop
        Target {
            label: "macOS (Apple Silicon)",
            platform: Platform::Desktop,
            rust_target: "aarch64-apple-darwin",
            build_args: &["tauri", "build", "--target", "aarch64-apple-darwin"],
        },
        Target {
            label: "macOS (Intel)",
            platform: Platform::Desktop,
            rust_target: "x86_64-apple-darwin",
            build_args: &["tauri", "build", "--target", "x86_64-apple-darwin"],
        },
        Target {
            label: "macOS (Universal)",
            platform: Platform::Desktop,
            rust_target: "universal-apple-darwin",
            build_args: &["tauri", "build", "--target", "universal-apple-darwin"],
        },
        Target {
            label: "Linux (x86_64)",
            platform: Platform::Desktop,
            rust_target: "x86_64-unknown-linux-gnu",
            build_args: &["tauri", "build"],
        },
        Target {
            label: "Windows (x86_64)",
            platform: Platform::Desktop,
            rust_target: "x86_64-pc-windows-msvc",
            build_args: &["tauri", "build"],
        },
        // Android
        Target {
            label: "Android (arm64-v8a)",
            platform: Platform::Android,
            rust_target: "aarch64-linux-android",
            build_args: &["tauri", "android", "build", "--target", "aarch64"],
        },
        Target {
            label: "Android (armeabi-v7a)",
            platform: Platform::Android,
            rust_target: "armv7-linux-androideabi",
            build_args: &["tauri", "android", "build", "--target", "armv7"],
        },
        Target {
            label: "Android (x86_64, emulator)",
            platform: Platform::Android,
            rust_target: "x86_64-linux-android",
            build_args: &["tauri", "android", "build", "--target", "x86_64"],
        },
        // iOS (Tauri uses short target names: aarch64, aarch64-sim, x86_64)
        Target {
            label: "iOS (device)",
            platform: Platform::Ios,
            rust_target: "aarch64-apple-ios",
            build_args: &["tauri", "ios", "build", "--target", "aarch64"],
        },
        Target {
            label: "iOS (simulator, ARM)",
            platform: Platform::Ios,
            rust_target: "aarch64-apple-ios-sim",
            build_args: &["tauri", "ios", "build", "--target", "aarch64-sim"],
        },
        Target {
            label: "iOS (simulator, Intel)",
            platform: Platform::Ios,
            rust_target: "x86_64-apple-ios",
            build_args: &["tauri", "ios", "build", "--target", "x86_64"],
        },
    ]
}

// ── Prerequisite checks ──────────────────────────────────────────────

struct PrereqStatus {
    label: String,
    ok: bool,
    detail: String,
}

fn check_rust_target_installed(triple: &str) -> bool {
    runner::run_silent(
        &std::env::current_dir().unwrap_or_default(),
        "rustup",
        &["target", "list", "--installed"],
    )
    .map(|out| out.lines().any(|l| l.trim() == triple))
    .unwrap_or(false)
}

fn check_android_sdk() -> PrereqStatus {
    let home = std::env::var("ANDROID_HOME").or_else(|_| std::env::var("ANDROID_SDK_ROOT"));
    match home {
        Ok(path) => {
            let exists = std::path::Path::new(&path).exists();
            PrereqStatus {
                label: "Android SDK".into(),
                ok: exists,
                detail: if exists {
                    path
                } else {
                    format!("{} (path does not exist)", path)
                },
            }
        }
        Err(_) => PrereqStatus {
            label: "Android SDK".into(),
            ok: false,
            detail: "ANDROID_HOME not set".into(),
        },
    }
}

fn check_android_ndk() -> PrereqStatus {
    let home = std::env::var("ANDROID_HOME").unwrap_or_default();
    let ndk_dir = std::path::Path::new(&home).join("ndk");
    if ndk_dir.exists() {
        // Find the latest version directory
        let version = std::fs::read_dir(&ndk_dir).ok().and_then(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
                .filter_map(|e| e.file_name().into_string().ok())
                .max()
        });
        match version {
            Some(v) => PrereqStatus {
                label: "Android NDK".into(),
                ok: true,
                detail: v,
            },
            None => PrereqStatus {
                label: "Android NDK".into(),
                ok: false,
                detail: "ndk/ exists but no version found".into(),
            },
        }
    } else {
        PrereqStatus {
            label: "Android NDK".into(),
            ok: false,
            detail: "not installed".into(),
        }
    }
}

fn check_java_version() -> PrereqStatus {
    match runner::run_silent(
        &std::env::current_dir().unwrap_or_default(),
        "java",
        &["-version"],
    ) {
        // java -version outputs to stderr, but run_silent captures stdout.
        // Some JVMs output to stdout, some to stderr. Try parsing both.
        Ok(out) => {
            let version_line = out.lines().next().unwrap_or("").to_string();
            PrereqStatus {
                label: "Java".into(),
                ok: true,
                detail: version_line,
            }
        }
        Err(_) => {
            // Try capturing stderr directly
            let output = std::process::Command::new("java").arg("-version").output();
            match output {
                Ok(o) => {
                    let stderr = String::from_utf8_lossy(&o.stderr);
                    let line = stderr.lines().next().unwrap_or("").to_string();
                    PrereqStatus {
                        label: "Java".into(),
                        ok: o.status.success(),
                        detail: if line.is_empty() {
                            "installed (version unknown)".into()
                        } else {
                            line
                        },
                    }
                }
                Err(_) => PrereqStatus {
                    label: "Java".into(),
                    ok: false,
                    detail: "not found".into(),
                },
            }
        }
    }
}

fn check_xcode() -> PrereqStatus {
    match runner::run_silent(
        &std::env::current_dir().unwrap_or_default(),
        "xcodebuild",
        &["-version"],
    ) {
        Ok(out) => {
            let version = out.lines().next().unwrap_or("installed").to_string();
            PrereqStatus {
                label: "Xcode".into(),
                ok: true,
                detail: version,
            }
        }
        Err(_) => PrereqStatus {
            label: "Xcode".into(),
            ok: false,
            detail: "not installed (xcodebuild not found)".into(),
        },
    }
}

fn check_tauri_cli() -> PrereqStatus {
    match runner::run_silent(
        &std::env::current_dir().unwrap_or_default(),
        "cargo",
        &["tauri", "--version"],
    ) {
        Ok(out) => PrereqStatus {
            label: "Tauri CLI".into(),
            ok: true,
            detail: out.trim().to_string(),
        },
        Err(_) => PrereqStatus {
            label: "Tauri CLI".into(),
            ok: false,
            detail: "not installed (cargo install tauri-cli)".into(),
        },
    }
}

fn prereqs_for_platform(platform: Platform, targets: &[&Target]) -> Vec<PrereqStatus> {
    let mut checks = vec![check_tauri_cli()];

    match platform {
        Platform::Desktop => {
            // Check Rust targets
            for t in targets {
                let installed = check_rust_target_installed(t.rust_target);
                checks.push(PrereqStatus {
                    label: format!("Rust target {}", t.rust_target),
                    ok: installed,
                    detail: if installed {
                        "installed".into()
                    } else {
                        format!("run: rustup target add {}", t.rust_target)
                    },
                });
            }
        }
        Platform::Android => {
            checks.push(check_android_sdk());
            checks.push(check_android_ndk());
            checks.push(check_java_version());
            for t in targets {
                let installed = check_rust_target_installed(t.rust_target);
                checks.push(PrereqStatus {
                    label: format!("Rust target {}", t.rust_target),
                    ok: installed,
                    detail: if installed {
                        "installed".into()
                    } else {
                        format!("run: rustup target add {}", t.rust_target)
                    },
                });
            }
        }
        Platform::Ios => {
            checks.push(check_xcode());
            for t in targets {
                let installed = check_rust_target_installed(t.rust_target);
                checks.push(PrereqStatus {
                    label: format!("Rust target {}", t.rust_target),
                    ok: installed,
                    detail: if installed {
                        "installed".into()
                    } else {
                        format!("run: rustup target add {}", t.rust_target)
                    },
                });
            }
        }
    }

    checks
}

fn display_prereqs(checks: &[PrereqStatus]) -> bool {
    let all_ok = checks.iter().all(|c| c.ok);
    for check in checks {
        if check.ok {
            eprintln!(
                "    {} {:30} {}",
                "●".green(),
                check.label,
                check.detail.dimmed()
            );
        } else {
            eprintln!(
                "    {} {:30} {}",
                "●".red(),
                check.label,
                check.detail.red()
            );
        }
    }
    all_ok
}

// ── CLI commands ─────────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum BuildCommand {
    /// Run cargo check (fast compile check, no codegen)
    Check,
    /// Full release build (cargo tauri build) — desktop only
    Release,
    /// Interactive platform build wizard
    Platform,
}

pub fn execute(cmd: &BuildCommand, ctx: &ProjectContext) -> Result<()> {
    match cmd {
        BuildCommand::Check => run_check(ctx),
        BuildCommand::Release => run_release(ctx),
        BuildCommand::Platform => run_platform_wizard(ctx),
    }
}

// ── Existing commands (check / release) ──────────────────────────────

fn run_check(ctx: &ProjectContext) -> Result<()> {
    let steps: &[(&str, &str, &[&str])] = &[
        ("Rust cargo check", "cargo", &["check"]),
        ("Frontend type check", "npx", &["vue-tsc", "-b"]),
    ];

    for (i, (label, prog, args)) in steps.iter().enumerate() {
        output::step(i + 1, steps.len(), label);
        let dir = if *prog == "cargo" {
            &ctx.tauri_dir
        } else {
            &ctx.root
        };
        runner::run_step(dir, prog, args)?;
        output::success(&format!("{} passed", label));
    }

    output::blank();
    output::success("All checks passed!");
    Ok(())
}

fn run_release(ctx: &ProjectContext) -> Result<()> {
    output::header("Building release");
    output::info("This will compile the full Tauri app bundle...");
    output::faint("(includes vue-tsc + vite build + cargo build --release)");
    output::blank();

    runner::run_step(&ctx.root, "cargo", &["tauri", "build"])?;

    output::blank();
    output::success("Release build complete!");
    output::faint("Bundle output: src-tauri/target/release/bundle/");
    Ok(())
}

// ── Interactive platform build wizard ────────────────────────────────

fn run_platform_wizard(ctx: &ProjectContext) -> Result<()> {
    let theme = ColorfulTheme::default();
    let targets = all_targets();

    // ── Step 1: Select platform ──────────────────────────────────────

    output::header("Platform build wizard");
    output::blank();

    let platforms = [
        "Desktop  (macOS / Linux / Windows)",
        "Android  (APK / AAB)",
        "iOS      (Xcode project)",
    ];
    let platform_sel = Select::with_theme(&theme)
        .with_prompt("Select platform")
        .items(&platforms)
        .default(0)
        .interact_opt()?;

    let platform = match platform_sel {
        Some(0) => Platform::Desktop,
        Some(1) => Platform::Android,
        Some(2) => Platform::Ios,
        _ => {
            output::info("Cancelled");
            return Ok(());
        }
    };

    // ── Step 2: Select targets within platform ───────────────────────

    let platform_targets: Vec<&Target> =
        targets.iter().filter(|t| t.platform == platform).collect();
    let target_labels: Vec<&str> = platform_targets.iter().map(|t| t.label).collect();

    output::blank();

    let selected_indices = if platform_targets.len() == 1 {
        // Only one target — auto-select
        output::info(&format!("Target: {}", platform_targets[0].label));
        vec![0]
    } else {
        // Default to first item checked
        let defaults: Vec<bool> = (0..platform_targets.len()).map(|i| i == 0).collect();
        let sel = MultiSelect::with_theme(&theme)
            .with_prompt("Select targets (space to toggle, enter to confirm)")
            .items(&target_labels)
            .defaults(&defaults)
            .interact_opt()?;

        match sel {
            Some(s) if !s.is_empty() => s,
            _ => {
                output::info("No targets selected — cancelled");
                return Ok(());
            }
        }
    };

    let selected_targets: Vec<&Target> = selected_indices
        .iter()
        .map(|&i| platform_targets[i])
        .collect();

    // ── Step 3: Build profile ────────────────────────────────────────

    output::blank();

    let profiles = [
        "Release  (optimized, slower build)",
        "Debug    (fast build, larger binary)",
    ];
    let profile_sel = Select::with_theme(&theme)
        .with_prompt("Build profile")
        .items(&profiles)
        .default(0)
        .interact_opt()?;

    let is_debug = match profile_sel {
        Some(0) => false,
        Some(1) => true,
        _ => {
            output::info("Cancelled");
            return Ok(());
        }
    };

    // ── Step 4: Prerequisite check ───────────────────────────────────

    output::blank();
    output::header("Checking prerequisites");
    output::blank();

    let prereqs = prereqs_for_platform(platform, &selected_targets);
    let all_ok = display_prereqs(&prereqs);

    output::blank();

    if !all_ok {
        output::error("Some prerequisites are missing — fix the issues above before building");
        output::faint("Install missing Rust targets with: rustup target add <triple>");
        return Ok(());
    }

    output::success("All prerequisites met");

    // ── Step 5: Confirmation ─────────────────────────────────────────

    output::blank();
    eprintln!("  {}", "Build summary".bold());
    eprintln!(
        "    {:>12}  {}",
        "Platform".dimmed(),
        style(&platform).cyan()
    );
    for t in &selected_targets {
        eprintln!("    {:>12}  {}", "Target".dimmed(), t.label);
    }
    eprintln!(
        "    {:>12}  {}",
        "Profile".dimmed(),
        if is_debug {
            style("debug").yellow()
        } else {
            style("release").green()
        }
    );
    output::blank();

    let confirmed = Confirm::with_theme(&theme)
        .with_prompt("Start build?")
        .default(true)
        .interact_opt()?;

    if confirmed != Some(true) {
        output::info("Build cancelled");
        return Ok(());
    }

    // ── Step 6: Execute builds ───────────────────────────────────────

    output::blank();
    let total = selected_targets.len();

    for (i, target) in selected_targets.iter().enumerate() {
        output::step(i + 1, total, &format!("Building {}", target.label));
        output::blank();

        let mut args: Vec<&str> = target.build_args.to_vec();
        if is_debug {
            args.push("--debug");
        }

        let dir = &ctx.root;
        let result = runner::run_step(dir, "cargo", &args);

        match result {
            Ok(()) => {
                output::blank();
                output::success(&format!("{} built successfully", target.label));
            }
            Err(e) => {
                output::blank();
                output::error(&format!("{} build failed: {}", target.label, e));
                if total > 1 {
                    let skip = Confirm::with_theme(&theme)
                        .with_prompt("Continue with remaining targets?")
                        .default(true)
                        .interact()?;
                    if !skip {
                        bail!("Build aborted");
                    }
                } else {
                    bail!("Build failed");
                }
            }
        }
        output::blank();
    }

    // ── Done ─────────────────────────────────────────────────────────

    output::blank();
    output::success(&format!("All {} target(s) built!", selected_targets.len()));

    // Print output locations
    output::blank();
    eprintln!("  {}", "Build artifacts".bold());
    for target in &selected_targets {
        let location = match target.platform {
            Platform::Desktop => "src-tauri/target/release/bundle/",
            Platform::Android => "src-tauri/gen/android/app/build/outputs/",
            Platform::Ios => "src-tauri/gen/apple/build/",
        };
        eprintln!("    {:>28}  {}", target.label.dimmed(), location);
    }
    output::blank();

    Ok(())
}
