use anyhow::{bail, Result};
use clap::Subcommand;
use dialoguer::{theme::ColorfulTheme, Select};
use owo_colors::OwoColorize;

use crate::context::ProjectContext;
use crate::output;
use crate::runner;

// ── Device types ─────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct Device {
    /// Display name (e.g. "iPhone 17 Pro", "Pixel_9_Pro")
    name: String,
    /// Additional context (iOS runtime, Android serial, etc.)
    detail: String,
    /// State (Booted, Shutdown, device, emulator, etc.)
    state: String,
    /// Unique ID (UDID for iOS, serial for Android)
    id: String,
}

// ── CLI commands ─────────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum RunCommand {
    /// Run on desktop (cargo tauri dev)
    Desktop {
        /// Build in release mode
        #[arg(long)]
        release: bool,
    },

    /// Run on an iOS simulator or connected device
    Ios {
        /// Device name to run on (skip selection prompt)
        #[arg(short, long)]
        device: Option<String>,

        /// Open in Xcode instead of running directly
        #[arg(short, long)]
        open: bool,

        /// Build in release mode
        #[arg(long)]
        release: bool,
    },

    /// Run on an Android emulator or connected device
    Android {
        /// Device name to run on (skip selection prompt)
        #[arg(short, long)]
        device: Option<String>,

        /// Open in Android Studio instead of running directly
        #[arg(short, long)]
        open: bool,

        /// Build in release mode
        #[arg(long)]
        release: bool,
    },
}

pub fn execute(cmd: &RunCommand, ctx: &ProjectContext) -> Result<()> {
    match cmd {
        RunCommand::Desktop { release } => run_desktop(ctx, *release),
        RunCommand::Ios {
            device,
            open,
            release,
        } => run_ios(ctx, device.as_deref(), *open, *release),
        RunCommand::Android {
            device,
            open,
            release,
        } => run_android(ctx, device.as_deref(), *open, *release),
    }
}

// ── Desktop ──────────────────────────────────────────────────────────

fn run_desktop(ctx: &ProjectContext, release: bool) -> Result<()> {
    output::header("Starting desktop dev server");
    output::faint("Press Ctrl+C to stop");
    output::blank();

    let mut args = vec!["tauri", "dev"];
    if release {
        args.push("--release");
    }

    runner::run_step(&ctx.root, "cargo", &args)?;
    Ok(())
}

// ── iOS ──────────────────────────────────────────────────────────────

fn list_ios_devices() -> Result<Vec<Device>> {
    let cwd = std::env::current_dir().unwrap_or_default();
    let mut devices = Vec::new();

    // Connected physical devices via xcrun devicectl
    if let Ok(out) = runner::run_silent(&cwd, "xcrun", &["devicectl", "list", "devices"]) {
        // Parse the table output — look for lines with device info
        // Format varies, but typically: Name  Identifier  ...
        for line in out.lines() {
            let trimmed = line.trim();
            // Skip headers and separators
            if trimmed.is_empty()
                || trimmed.starts_with("--")
                || trimmed.starts_with("==")
                || trimmed.contains("Identifier")
                || trimmed.contains("No devices")
            {
                continue;
            }
            // Physical devices show up with a UDID-like pattern
            // We'll rely on the simctl list for simulators and just flag
            // physically connected devices differently
        }
    }

    // Simulators via xcrun simctl list
    let out = runner::run_silent(&cwd, "xcrun", &["simctl", "list", "devices", "available"])?;

    let mut current_runtime = String::new();
    for line in out.lines() {
        let trimmed = line.trim();

        // Runtime header: "-- iOS 18.5 --" or "-- iOS 26.0 --"
        if trimmed.starts_with("-- ") && trimmed.ends_with(" --") {
            current_runtime = trimmed
                .trim_start_matches("-- ")
                .trim_end_matches(" --")
                .to_string();
            continue;
        }

        // Device line: "    iPhone 17 Pro (UDID) (Booted)" or "(Shutdown)"
        if !current_runtime.is_empty() && trimmed.contains('(') {
            // Parse: "Name (UDID) (State)"
            if let Some((name, rest)) = trimmed.split_once(" (") {
                let parts: Vec<&str> = rest.split(") (").collect();
                if parts.len() >= 2 {
                    let udid = parts[0].trim_end_matches(')');
                    let state = parts[1].trim_end_matches(')');
                    devices.push(Device {
                        name: name.to_string(),
                        detail: current_runtime.clone(),
                        state: state.to_string(),
                        id: udid.to_string(),
                    });
                }
            }
        }
    }

    // Also check for physically connected iOS devices via xcrun xctrace
    if let Ok(out) = runner::run_silent(&cwd, "xcrun", &["xctrace", "list", "devices"]) {
        let mut in_devices_section = false;
        for line in out.lines() {
            let trimmed = line.trim();
            if trimmed == "== Devices ==" {
                in_devices_section = true;
                continue;
            }
            if trimmed.starts_with("== ") {
                in_devices_section = false;
                continue;
            }
            if in_devices_section && !trimmed.is_empty() {
                // Format: "Device Name (UDID)"
                // Skip entries that look like the Mac itself
                if trimmed.contains("Mac") || trimmed.contains("macOS") {
                    continue;
                }
                if let Some((name, rest)) = trimmed.rsplit_once(" (") {
                    let udid = rest.trim_end_matches(')');
                    // Only add if not already in simulator list
                    if !devices.iter().any(|d| d.id == udid) {
                        devices.push(Device {
                            name: name.to_string(),
                            detail: "Physical device".to_string(),
                            state: "Connected".to_string(),
                            id: udid.to_string(),
                        });
                    }
                }
            }
        }
    }

    Ok(devices)
}

fn run_ios(ctx: &ProjectContext, device: Option<&str>, open: bool, release: bool) -> Result<()> {
    output::header("Run on iOS");
    output::blank();

    let device_name = if let Some(name) = device {
        name.to_string()
    } else {
        // List devices and let user pick
        output::info("Scanning for iOS simulators and devices...");
        output::blank();

        let devices = list_ios_devices()?;
        if devices.is_empty() {
            bail!(
                "No iOS simulators or devices found.\n\
                 Install simulators via Xcode > Settings > Platforms."
            );
        }

        let selected = pick_device(&devices, "iOS")?;
        selected.name.clone()
    };

    output::info(&format!("Target: {}", device_name));
    output::faint("Press Ctrl+C to stop");
    output::blank();

    let mut args = vec!["tauri", "ios", "dev"];
    if open {
        args.push("--open");
    }
    if release {
        args.push("--release");
    }
    // Device name is a positional argument for `cargo tauri ios dev`
    let device_arg = device_name.clone();
    args.push(&device_arg);

    runner::run_step(&ctx.root, "cargo", &args)?;
    Ok(())
}

// ── Android ──────────────────────────────────────────────────────────

fn list_android_devices() -> Result<Vec<Device>> {
    let cwd = std::env::current_dir().unwrap_or_default();
    let mut devices = Vec::new();

    // Connected devices/emulators via adb
    if let Ok(out) = runner::run_silent(&cwd, "adb", &["devices", "-l"]) {
        for line in out.lines().skip(1) {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            // Format: "serial  state  key:value  key:value ..."
            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if parts.len() >= 2 && parts[1] == "device" {
                let serial = parts[0];
                let model = parts
                    .iter()
                    .find(|p| p.starts_with("model:"))
                    .map(|p| p.trim_start_matches("model:"))
                    .unwrap_or("Unknown");
                let transport = parts
                    .iter()
                    .find(|p| p.starts_with("transport_id:"))
                    .map(|p| p.trim_start_matches("transport_id:"))
                    .unwrap_or("");
                let is_emulator = serial.starts_with("emulator-");
                devices.push(Device {
                    name: serial.to_string(),
                    detail: model.to_string(),
                    state: if is_emulator {
                        "Running (emulator)".to_string()
                    } else {
                        "Connected".to_string()
                    },
                    id: transport.to_string(),
                });
            }
        }
    }

    // Available AVDs (not yet running)
    let emulator_path = std::env::var("ANDROID_HOME")
        .map(|h| format!("{}/emulator/emulator", h))
        .unwrap_or_else(|_| "emulator".to_string());

    if let Ok(out) = runner::run_silent(&cwd, &emulator_path, &["-list-avds"]) {
        for line in out.lines() {
            let avd = line.trim();
            if avd.is_empty() {
                continue;
            }
            // Skip if this AVD is already running (matches a running emulator)
            let already_running = devices
                .iter()
                .any(|d| d.state.contains("emulator") && d.detail.replace(' ', "_").contains(avd));
            if !already_running {
                devices.push(Device {
                    name: avd.to_string(),
                    detail: "AVD".to_string(),
                    state: "Not running".to_string(),
                    id: avd.to_string(),
                });
            }
        }
    }

    Ok(devices)
}

fn run_android(
    ctx: &ProjectContext,
    device: Option<&str>,
    open: bool,
    release: bool,
) -> Result<()> {
    output::header("Run on Android");
    output::blank();

    let device_name = if let Some(name) = device {
        name.to_string()
    } else {
        output::info("Scanning for Android emulators and devices...");
        output::blank();

        let devices = list_android_devices()?;
        if devices.is_empty() {
            bail!(
                "No Android emulators or devices found.\n\
                 Create an AVD via Android Studio > Virtual Device Manager,\n\
                 or connect a physical device with USB debugging enabled."
            );
        }

        let selected = pick_device(&devices, "Android")?;
        selected.name.clone()
    };

    output::info(&format!("Target: {}", device_name));
    output::faint("Press Ctrl+C to stop");
    output::blank();

    let mut args = vec!["tauri", "android", "dev"];
    if open {
        args.push("--open");
    }
    if release {
        args.push("--release");
    }
    let device_arg = device_name.clone();
    args.push(&device_arg);

    runner::run_step(&ctx.root, "cargo", &args)?;
    Ok(())
}

// ── Shared device picker ─────────────────────────────────────────────

fn pick_device(devices: &[Device], platform: &str) -> Result<Device> {
    let theme = ColorfulTheme::default();

    // Group and sort: booted/connected first, then by name
    let mut sorted: Vec<&Device> = devices.iter().collect();
    sorted.sort_by(|a, b| {
        let a_active = is_active_state(&a.state);
        let b_active = is_active_state(&b.state);
        b_active
            .cmp(&a_active)
            .then_with(|| a.detail.cmp(&b.detail))
            .then_with(|| a.name.cmp(&b.name))
    });

    // Build display labels
    let labels: Vec<String> = sorted
        .iter()
        .map(|d| {
            let state_indicator = if is_active_state(&d.state) {
                format!("{}", "●".green())
            } else {
                format!("{}", "○".dimmed())
            };
            format!(
                "{} {}  {}  {}",
                state_indicator,
                d.name,
                d.detail.dimmed(),
                d.state.dimmed(),
            )
        })
        .collect();

    // Find default selection (first booted/connected, or first item)
    let default = sorted
        .iter()
        .position(|d| is_active_state(&d.state))
        .unwrap_or(0);

    let selection = Select::with_theme(&theme)
        .with_prompt(format!("Select {} device", platform))
        .items(&labels)
        .default(default)
        .interact_opt()?;

    match selection {
        Some(i) => Ok(sorted[i].clone()),
        None => bail!("Cancelled"),
    }
}

fn is_active_state(state: &str) -> bool {
    matches!(
        state,
        "Booted" | "Connected" | "Running (emulator)" | "device"
    )
}
