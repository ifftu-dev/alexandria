//! Diagnostic file logger for iOS debugging.
//!
//! Writes timestamped lines to `diag.log` in the app's data directory.
//! This bypasses `os_log` / `NSLog` which don't reliably surface Rust
//! `log::info!` output in simulator builds.
//!
//! Usage:
//!   diag::init("/path/to/app_data");
//!   diag::log("some message");
//!
//! A Tauri command `read_diag_log` lets the frontend fetch the contents.

use std::io::Write;
use std::path::PathBuf;
use std::sync::OnceLock;

static DIAG_PATH: OnceLock<PathBuf> = OnceLock::new();

/// Set the diagnostic log file path. Call once during app setup.
pub fn init(app_data_dir: &std::path::Path) {
    let path = app_data_dir.join("diag.log");
    // Truncate previous log on each app launch
    let _ = std::fs::write(&path, format!("=== diag.log started ===\n"));
    let _ = DIAG_PATH.set(path);
}

/// Append a timestamped line to the diagnostic log.
///
/// This function must NEVER panic — it is called from the panic hook.
/// A panic here would cause a double-panic → unconditional abort().
pub fn log(msg: &str) {
    if let Some(path) = DIAG_PATH.get() {
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(path)
        {
            let now = chrono::Utc::now().format("%H:%M:%S%.3f");
            let _ = writeln!(f, "[{now}] {msg}");
        }
    }
    // Best-effort stderr — use write! to avoid panic on broken pipe.
    // eprintln! panics if stderr is unavailable (e.g., cargo tauri dev
    // after the process has been re-parented). This caused the SIGABRT
    // crash: p2p_start → diag::log → eprintln! → panic → panic_hook
    // → diag::log → eprintln! → double-panic → abort().
    let _ = std::io::stderr().write_all(format!("[diag] {msg}\n").as_bytes());
}

/// Read the diagnostic log contents (for the Tauri command).
pub fn read() -> String {
    DIAG_PATH
        .get()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_else(|| "(diag log not initialized)".to_string())
}

/// Install a panic hook that writes to the diagnostic log before aborting.
///
/// The hook catches secondary panics to prevent double-panic → abort().
pub fn install_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // Wrap in catch_unwind so a failure in the hook itself
        // (e.g., formatting the PanicInfo) doesn't cause a double-panic.
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let msg = format!("PANIC: {info}");
            log(&msg);
        }));
        default_hook(info);
    }));
}
