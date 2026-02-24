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
    /// Short name for CLI --target flag (e.g. "arm64", "sim", "x86_64")
    short_name: &'static str,
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
            short_name: "mac-arm64",
            platform: Platform::Desktop,
            rust_target: "aarch64-apple-darwin",
            build_args: &["tauri", "build", "--target", "aarch64-apple-darwin"],
        },
        Target {
            label: "macOS (Intel)",
            short_name: "mac-x64",
            platform: Platform::Desktop,
            rust_target: "x86_64-apple-darwin",
            build_args: &["tauri", "build", "--target", "x86_64-apple-darwin"],
        },
        Target {
            label: "macOS (Universal)",
            short_name: "mac-universal",
            platform: Platform::Desktop,
            rust_target: "universal-apple-darwin",
            build_args: &["tauri", "build", "--target", "universal-apple-darwin"],
        },
        Target {
            label: "Linux (x86_64)",
            short_name: "linux-x64",
            platform: Platform::Desktop,
            rust_target: "x86_64-unknown-linux-gnu",
            build_args: &["tauri", "build"],
        },
        Target {
            label: "Windows (x86_64)",
            short_name: "win-x64",
            platform: Platform::Desktop,
            rust_target: "x86_64-pc-windows-msvc",
            build_args: &["tauri", "build"],
        },
        // Android
        Target {
            label: "Android (arm64-v8a)",
            short_name: "arm64",
            platform: Platform::Android,
            rust_target: "aarch64-linux-android",
            build_args: &["tauri", "android", "build", "--target", "aarch64"],
        },
        Target {
            label: "Android (armeabi-v7a)",
            short_name: "armv7",
            platform: Platform::Android,
            rust_target: "armv7-linux-androideabi",
            build_args: &["tauri", "android", "build", "--target", "armv7"],
        },
        Target {
            label: "Android (x86_64, emulator)",
            short_name: "x86_64",
            platform: Platform::Android,
            rust_target: "x86_64-linux-android",
            build_args: &["tauri", "android", "build", "--target", "x86_64"],
        },
        // iOS (Tauri uses short target names: aarch64, aarch64-sim, x86_64)
        Target {
            label: "iOS (device)",
            short_name: "device",
            platform: Platform::Ios,
            rust_target: "aarch64-apple-ios",
            build_args: &["tauri", "ios", "build", "--target", "aarch64"],
        },
        Target {
            label: "iOS (simulator, ARM)",
            short_name: "sim-arm64",
            platform: Platform::Ios,
            rust_target: "aarch64-apple-ios-sim",
            build_args: &["tauri", "ios", "build", "--target", "aarch64-sim"],
        },
        Target {
            label: "iOS (simulator, Intel)",
            short_name: "sim-x64",
            platform: Platform::Ios,
            rust_target: "x86_64-apple-ios",
            build_args: &["tauri", "ios", "build", "--target", "x86_64"],
        },
    ]
}

/// Return the default target for a platform based on the current host.
fn default_targets_for_platform(platform: Platform) -> Vec<&'static str> {
    match platform {
        Platform::Desktop => {
            if cfg!(target_arch = "aarch64") {
                vec!["mac-arm64"]
            } else if cfg!(target_arch = "x86_64") {
                if cfg!(target_os = "macos") {
                    vec!["mac-x64"]
                } else if cfg!(target_os = "linux") {
                    vec!["linux-x64"]
                } else {
                    vec!["win-x64"]
                }
            } else {
                vec![]
            }
        }
        Platform::Android => vec!["arm64"],
        Platform::Ios => {
            if cfg!(target_arch = "aarch64") {
                vec!["sim-arm64"]
            } else {
                vec!["sim-x64"]
            }
        }
    }
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
        Ok(out) => {
            let version_line = out.lines().next().unwrap_or("").to_string();
            PrereqStatus {
                label: "Java".into(),
                ok: true,
                detail: version_line,
            }
        }
        Err(_) => {
            // java -version outputs to stderr — try capturing it directly
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
    /// Run cargo check + vue-tsc (fast compile check, no codegen)
    Check,

    /// Full release build for the current host (cargo tauri build)
    Release,

    /// Interactive platform build wizard (prompts for platform, targets, profile)
    Platform,

    /// Build for desktop (macOS / Linux / Windows)
    ///
    /// Targets: mac-arm64, mac-x64, mac-universal, linux-x64, win-x64.
    /// Defaults to the current host architecture.
    Desktop {
        /// Targets to build (e.g. mac-arm64 mac-x64). Omit for host default.
        #[arg(short, long, num_args = 1..)]
        target: Vec<String>,

        /// Build in debug mode (faster compile, larger binary)
        #[arg(short, long)]
        debug: bool,
    },

    /// Build for Android (APK / AAB)
    ///
    /// Targets: arm64, armv7, x86_64.
    /// Defaults to arm64.
    Android {
        /// Targets to build (e.g. arm64 armv7). Omit for arm64.
        #[arg(short, long, num_args = 1..)]
        target: Vec<String>,

        /// Build in debug mode
        #[arg(short, long)]
        debug: bool,
    },

    /// Build for iOS (Xcode archive)
    ///
    /// Targets: device, sim-arm64, sim-x64.
    /// Defaults to the simulator for the current host architecture.
    Ios {
        /// Targets to build (e.g. device sim-arm64). Omit for simulator default.
        #[arg(short, long, num_args = 1..)]
        target: Vec<String>,

        /// Build in debug mode
        #[arg(short, long)]
        debug: bool,
    },

    /// Build for all platforms (desktop + android + ios)
    All {
        /// Build in debug mode
        #[arg(short, long)]
        debug: bool,
    },

    /// List all available build targets
    List,
}

pub fn execute(cmd: &BuildCommand, ctx: &ProjectContext) -> Result<()> {
    match cmd {
        BuildCommand::Check => run_check(ctx),
        BuildCommand::Release => run_release(ctx),
        BuildCommand::Platform => run_platform_wizard(ctx),
        BuildCommand::Desktop { target, debug } => {
            run_platform_build(ctx, Platform::Desktop, target, *debug)
        }
        BuildCommand::Android { target, debug } => {
            run_platform_build(ctx, Platform::Android, target, *debug)
        }
        BuildCommand::Ios { target, debug } => {
            run_platform_build(ctx, Platform::Ios, target, *debug)
        }
        BuildCommand::All { debug } => run_all_platforms(ctx, *debug),
        BuildCommand::List => run_list(),
    }
}

// ── List targets ─────────────────────────────────────────────────────

fn run_list() -> Result<()> {
    let targets = all_targets();
    let mut current_platform: Option<Platform> = None;

    for t in &targets {
        if current_platform != Some(t.platform) {
            if current_platform.is_some() {
                output::blank();
            }
            output::header(&format!("{}", t.platform));
            current_platform = Some(t.platform);
        }
        eprintln!(
            "    {:16} {:30} {}",
            t.short_name.cyan(),
            t.label,
            t.rust_target.dimmed()
        );
    }
    output::blank();
    output::faint("Use --target <name> to select specific targets, e.g.:");
    output::faint("  alex build desktop --target mac-arm64 mac-x64");
    output::faint("  alex build android --target arm64 armv7");
    output::faint("  alex build ios --target device sim-arm64");
    output::blank();

    Ok(())
}

// ── Non-interactive platform build ───────────────────────────────────

fn resolve_targets<'a>(
    platform: Platform,
    requested: &[String],
    all: &'a [Target],
) -> Result<Vec<&'a Target>> {
    let platform_targets: Vec<&Target> = all.iter().filter(|t| t.platform == platform).collect();

    if requested.is_empty() {
        // Use defaults for this platform
        let defaults = default_targets_for_platform(platform);
        if defaults.is_empty() {
            bail!(
                "No default target for {} on this host. Use --target to specify one.\n\
                 Run `alex build list` to see available targets.",
                platform
            );
        }
        let resolved: Vec<&Target> = platform_targets
            .into_iter()
            .filter(|t| defaults.contains(&t.short_name))
            .collect();
        if resolved.is_empty() {
            bail!(
                "Default target(s) {:?} not available for {}",
                defaults,
                platform
            );
        }
        return Ok(resolved);
    }

    // Resolve requested target names
    let mut resolved = Vec::new();
    for name in requested {
        let found = platform_targets
            .iter()
            .find(|t| t.short_name == name.as_str());
        match found {
            Some(t) => resolved.push(*t),
            None => {
                let available: Vec<&str> = platform_targets.iter().map(|t| t.short_name).collect();
                bail!(
                    "Unknown {} target: '{}'\nAvailable targets: {}",
                    platform,
                    name,
                    available.join(", ")
                );
            }
        }
    }

    Ok(resolved)
}

fn run_platform_build(
    ctx: &ProjectContext,
    platform: Platform,
    requested_targets: &[String],
    is_debug: bool,
) -> Result<()> {
    let all = all_targets();
    let selected = resolve_targets(platform, requested_targets, &all)?;
    let selected_refs: Vec<&Target> = selected.iter().copied().collect();

    let profile_label = if is_debug { "debug" } else { "release" };
    output::header(&format!(
        "Building {} ({} profile)",
        platform, profile_label
    ));
    output::blank();

    // Prerequisite check
    output::info("Checking prerequisites...");
    output::blank();
    let prereqs = prereqs_for_platform(platform, &selected_refs);
    let all_ok = display_prereqs(&prereqs);
    output::blank();

    if !all_ok {
        bail!("Prerequisites not met — install the missing items listed above");
    }

    // Build summary
    output::info(&format!(
        "Targets: {}",
        selected
            .iter()
            .map(|t| t.label)
            .collect::<Vec<_>>()
            .join(", ")
    ));
    output::blank();

    // Execute builds
    execute_builds(ctx, &selected, is_debug)?;

    Ok(())
}

fn run_all_platforms(ctx: &ProjectContext, is_debug: bool) -> Result<()> {
    let profile_label = if is_debug { "debug" } else { "release" };
    output::header(&format!(
        "Building all platforms ({} profile)",
        profile_label
    ));
    output::blank();

    let all = all_targets();
    let mut all_selected: Vec<&Target> = Vec::new();
    let platforms = [Platform::Desktop, Platform::Android, Platform::Ios];

    for platform in &platforms {
        let defaults = default_targets_for_platform(*platform);
        if defaults.is_empty() {
            output::warning(&format!(
                "Skipping {} — no default target for this host",
                platform
            ));
            continue;
        }

        let targets: Vec<&Target> = all
            .iter()
            .filter(|t| t.platform == *platform && defaults.contains(&t.short_name))
            .collect();
        all_selected.extend(targets);
    }

    if all_selected.is_empty() {
        bail!("No targets available for any platform on this host");
    }

    // Check all prerequisites
    output::info("Checking prerequisites...");
    output::blank();

    let mut all_prereqs_ok = true;
    for platform in &platforms {
        let platform_targets: Vec<&Target> = all_selected
            .iter()
            .filter(|t| t.platform == *platform)
            .copied()
            .collect();
        if platform_targets.is_empty() {
            continue;
        }
        let prereqs = prereqs_for_platform(*platform, &platform_targets);
        if !display_prereqs(&prereqs) {
            all_prereqs_ok = false;
        }
    }
    output::blank();

    if !all_prereqs_ok {
        bail!("Prerequisites not met — install the missing items listed above");
    }

    // Build summary
    output::info(&format!(
        "Targets: {}",
        all_selected
            .iter()
            .map(|t| t.label)
            .collect::<Vec<_>>()
            .join(", ")
    ));
    output::blank();

    execute_builds(ctx, &all_selected, is_debug)?;

    Ok(())
}

// ── Shared build executor ────────────────────────────────────────────

fn execute_builds(ctx: &ProjectContext, targets: &[&Target], is_debug: bool) -> Result<()> {
    let total = targets.len();
    let mut succeeded = 0;
    let mut failed: Vec<&str> = Vec::new();

    for (i, target) in targets.iter().enumerate() {
        output::step(i + 1, total, &format!("Building {}", target.label));
        output::blank();

        let mut args: Vec<&str> = target.build_args.to_vec();
        if is_debug {
            args.push("--debug");
        }

        let result = runner::run_step(&ctx.root, "cargo", &args);

        match result {
            Ok(()) => {
                output::blank();
                output::success(&format!("{} built successfully", target.label));
                succeeded += 1;
            }
            Err(e) => {
                output::blank();
                output::error(&format!("{} build failed: {}", target.label, e));
                failed.push(target.label);
            }
        }
        output::blank();
    }

    // Summary
    output::blank();
    if failed.is_empty() {
        output::success(&format!("All {} target(s) built!", total));
    } else {
        output::success(&format!("{}/{} target(s) built", succeeded, total));
        output::error(&format!("Failed: {}", failed.join(", ")));
    }

    // Print output locations
    output::blank();
    eprintln!("  {}", "Build artifacts".bold());
    for target in targets {
        if failed.contains(&target.label) {
            continue;
        }
        let location = match target.platform {
            Platform::Desktop => {
                if is_debug {
                    "src-tauri/target/debug/bundle/"
                } else {
                    "src-tauri/target/release/bundle/"
                }
            }
            Platform::Android => "src-tauri/gen/android/app/build/outputs/",
            Platform::Ios => "src-tauri/gen/apple/build/",
        };
        eprintln!("    {:>28}  {}", target.label.dimmed(), location);
    }
    output::blank();

    if !failed.is_empty() {
        bail!("{} target(s) failed", failed.len());
    }

    Ok(())
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
        output::info(&format!("Target: {}", platform_targets[0].label));
        vec![0]
    } else {
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
    execute_builds(ctx, &selected_targets, is_debug)?;

    Ok(())
}
