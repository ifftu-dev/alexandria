//! Install the `alexandria` command-line tool so it is available on `PATH`.
//!
//! Release bundles ship the CLI beside the app binary (`externalBin` in the
//! desktop config overlays), and installing means **symlinking** that bundled
//! binary into a directory on `PATH` rather than copying it. The symlink is
//! what makes the auto-updater work on the CLI for free: an update replaces
//! the contents of `Alexandria.app`, and the link — whose target path inside
//! the bundle is unchanged — resolves to the new binary immediately. Copying
//! would strand the old version until the user reinstalled by hand.
//!
//! Development builds have no bundled CLI (the overlay that adds it is only
//! applied by the release workflows, so `tauri dev` does not pay for a CLI
//! release build on every run). There, installing compiles the CLI from the
//! working tree with `cargo install`, which is the only thing that can produce
//! an up-to-date binary from a source checkout.
//!
//! Windows is deliberately unimplemented rather than half-implemented: it has
//! no conventional `PATH` directory to link into, and symlink creation there
//! needs elevation or developer mode. The command reports that plainly instead
//! of copying a binary that the updater would then leave stale.

use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

/// Preferred install directory: on the default `PATH` for every shell.
#[cfg(unix)]
const INSTALL_DIR: &str = "/usr/local/bin";

/// Fallback when `/usr/local/bin` is not writable.
///
/// On a stock macOS install `/usr/local/bin` is root-owned, so linking there
/// needs `sudo`. Rather than dead-ending, fall back to the per-user directory
/// — but only when it already exists, since creating it would put the binary
/// somewhere that is probably not on `PATH` and appear to succeed while
/// `alexandria` stayed "command not found".
#[cfg(unix)]
const FALLBACK_INSTALL_DIR: &str = ".local/bin";

/// The command name created on `PATH`.
const CLI_NAME: &str = "alexandria";

/// The sidecar's filename inside the bundle.
///
/// Distinct from the command name so the pair reads clearly where it lands:
/// on macOS the sidecar sits in `Contents/MacOS/` beside the app executable,
/// which Tauri names after the cargo binary (`alexandria-node`), not after
/// `productName`. The link created on `PATH` is still `alexandria`, so this
/// name never reaches users.
const BUNDLED_NAME: &str = "alexandria-cli";

/// Progress lines from a source build are streamed to the UI under this event,
/// because the build takes minutes and a silent spinner is indistinguishable
/// from a hang.
const PROGRESS_EVENT: &str = "cli-install://progress";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliInstallStatus {
    /// Whether a CLI is currently installed at the managed location.
    pub installed: bool,
    /// The managed install path, whether or not anything is there yet.
    pub install_path: String,
    /// What the installed entry currently points at, when it is a symlink.
    pub links_to: Option<String>,
    /// Whether this build ships a CLI beside the app binary.
    pub bundled_available: bool,
    /// Path of the bundled CLI, when there is one.
    pub bundled_path: Option<String>,
    /// Whether a source checkout was found, enabling the build-from-source path.
    pub source_available: bool,
    /// Whether the platform supports installing at all.
    pub supported: bool,
    /// True when the installed entry is a symlink into this app bundle, so an
    /// app update will carry the CLI with it.
    pub tracks_updates: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliInstallResult {
    pub path: String,
    /// `"bundled"` when linked to the shipped binary, `"source"` when built.
    pub source: String,
    pub tracks_updates: bool,
    pub message: String,
}

// ---- Discovery ----------------------------------------------------------

/// Platform filename for a given base name.
fn exe_name(base: &str) -> String {
    if cfg!(windows) {
        format!("{base}.exe")
    } else {
        base.to_string()
    }
}

fn cli_file_name() -> String {
    exe_name(CLI_NAME)
}

/// The CLI shipped beside the app binary, if this build has one.
fn bundled_cli() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let candidate = exe.parent()?.join(exe_name(BUNDLED_NAME));
    candidate.is_file().then_some(candidate)
}

/// Walk up from the running binary looking for the workspace that contains the
/// CLI crate. In `tauri dev` the executable sits in `<root>/target/debug/`, so
/// the crate is two levels up; the loop keeps this from depending on that.
fn source_root() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let mut dir = exe.parent()?.to_path_buf();
    loop {
        if dir.join("cli/Cargo.toml").is_file() {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// The per-user fallback directory, if it exists and we can write to it.
#[cfg(unix)]
fn writable_fallback_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let dir = PathBuf::from(home).join(FALLBACK_INSTALL_DIR);
    (dir.is_dir() && is_writable(&dir)).then_some(dir)
}

#[cfg(unix)]
fn is_writable(dir: &Path) -> bool {
    // Probe by creating and removing a file: permission bits alone do not
    // account for ownership, ACLs, or a read-only mount.
    let probe = dir.join(".alexandria-write-probe");
    match std::fs::File::create(&probe) {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

/// Where the link goes: the standard location when it is usable, otherwise the
/// per-user fallback. An existing managed link wins over both, so a reinstall
/// updates in place rather than leaving two copies on `PATH`.
#[cfg(unix)]
fn install_dir() -> PathBuf {
    let preferred = PathBuf::from(INSTALL_DIR);
    if preferred.join(CLI_NAME).symlink_metadata().is_ok() {
        return preferred;
    }
    if let Some(fallback) = writable_fallback_dir() {
        if fallback.join(CLI_NAME).symlink_metadata().is_ok() {
            return fallback;
        }
    }
    if preferred.is_dir() && is_writable(&preferred) {
        return preferred;
    }
    writable_fallback_dir().unwrap_or(preferred)
}

#[cfg(unix)]
fn install_path() -> PathBuf {
    install_dir().join(CLI_NAME)
}

#[cfg(not(unix))]
fn install_path() -> PathBuf {
    PathBuf::from(cli_file_name())
}

/// Resolve what an installed entry points at. `read_link` rather than
/// `canonicalize` so a symlink whose target has gone missing still reports the
/// path it was aiming at, which is the useful thing to show.
fn link_target(path: &Path) -> Option<PathBuf> {
    std::fs::read_link(path).ok()
}

// ---- Status -------------------------------------------------------------

#[tauri::command]
pub async fn cli_install_status() -> Result<CliInstallStatus, String> {
    let path = install_path();
    let bundled = bundled_cli();
    let links_to = link_target(&path);

    // "Tracks updates" means the link resolves into the bundled binary, which
    // is the only arrangement an app update refreshes automatically.
    let tracks_updates = match (&links_to, &bundled) {
        (Some(target), Some(bundled)) => target == bundled,
        _ => false,
    };

    Ok(CliInstallStatus {
        installed: path.exists() || links_to.is_some(),
        install_path: path.display().to_string(),
        links_to: links_to.map(|p| p.display().to_string()),
        bundled_available: bundled.is_some(),
        bundled_path: bundled.map(|p| p.display().to_string()),
        source_available: source_root().is_some(),
        supported: cfg!(unix),
        tracks_updates,
    })
}

// ---- Install ------------------------------------------------------------

#[tauri::command]
pub async fn cli_install(app: AppHandle) -> Result<CliInstallResult, String> {
    if !cfg!(unix) {
        return Err(
            "Installing the CLI is only supported on macOS and Linux. On Windows, \
             add the app's install directory to PATH manually."
                .into(),
        );
    }

    if let Some(bundled) = bundled_cli() {
        return link_bundled(&app, &bundled);
    }

    let Some(root) = source_root() else {
        return Err(
            "This build does not ship the CLI and no source checkout was found next \
             to it, so there is nothing to install."
                .into(),
        );
    };

    build_from_source(app, root).await
}

#[cfg(unix)]
fn link_bundled(app: &AppHandle, bundled: &Path) -> Result<CliInstallResult, String> {
    use std::os::unix::fs::symlink;

    let dest = install_path();
    emit(
        app,
        &format!("Linking {} → {}", dest.display(), bundled.display()),
    );

    let dir = dest.parent().unwrap_or(Path::new(INSTALL_DIR));
    if !dir.is_dir() {
        return Err(format!(
            "{} does not exist. Create it and try again:\n  sudo mkdir -p {}",
            dir.display(),
            dir.display()
        ));
    }
    if !is_writable(dir) {
        return Err(format!(
            "{} is not writable, and no per-user fallback was available.\n\n\
             Either create ~/.local/bin and make sure it is on your PATH, or link \
             it yourself:\n  sudo ln -sf {} {}",
            dir.display(),
            bundled.display(),
            dest.display()
        ));
    }

    // Replace our own link, but never a real file: something else owns a
    // regular file at that path and clobbering it would destroy it silently.
    if dest.symlink_metadata().is_ok() {
        if link_target(&dest).is_none() {
            return Err(format!(
                "{} already exists and is not a symlink. Remove it first if you want \
                 the app to manage it:\n  sudo rm {}",
                dest.display(),
                dest.display()
            ));
        }
        std::fs::remove_file(&dest).map_err(|e| {
            format!(
                "Could not replace the existing link at {}: {e}",
                dest.display()
            )
        })?;
    }

    symlink(bundled, &dest).map_err(|e| {
        format!(
            "Could not create {}: {e}\n\nIf this is a permissions error, run:\n  \
             sudo ln -sf {} {}",
            dest.display(),
            bundled.display(),
            dest.display()
        )
    })?;

    emit(app, "Done.");
    Ok(CliInstallResult {
        path: dest.display().to_string(),
        source: "bundled".into(),
        tracks_updates: true,
        message: format!("Linked to the bundled CLI. App updates will update `{CLI_NAME}` too."),
    })
}

#[cfg(not(unix))]
fn link_bundled(_app: &AppHandle, _bundled: &Path) -> Result<CliInstallResult, String> {
    Err("Unsupported on this platform".into())
}

/// Compile and install the CLI from the working tree.
///
/// Only reachable from a development build. Runs on a blocking thread and
/// streams cargo's output, because a release build of the CLI pulls the whole
/// application library and takes minutes.
async fn build_from_source(app: AppHandle, root: PathBuf) -> Result<CliInstallResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        use std::io::{BufRead, BufReader};
        use std::process::{Command, Stdio};

        emit(
            &app,
            "No bundled CLI in this build — compiling from source. This takes a few minutes.",
        );

        let mut child = Command::new("cargo")
            .args(["install", "--path", "cli", "--locked"])
            .current_dir(&root)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                format!("Could not run cargo: {e}. Is the Rust toolchain installed and on PATH?")
            })?;

        // cargo writes progress to stderr and almost nothing to stdout, so the
        // interesting stream is stderr.
        if let Some(stderr) = child.stderr.take() {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                emit(&app, line.trim_end());
            }
        }

        let status = child
            .wait()
            .map_err(|e| format!("cargo install did not complete: {e}"))?;
        if !status.success() {
            return Err(format!(
                "cargo install failed with exit code {}. See the log above.",
                status.code().unwrap_or(-1)
            ));
        }

        // Mirror where `cargo install` actually put it: CARGO_HOME when set,
        // otherwise ~/.cargo. Reading the environment avoids taking a
        // dependency on `dirs` for a single path in a dev-only branch.
        let cargo_home = std::env::var_os("CARGO_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cargo")));
        let installed = cargo_home
            .map(|c| c.join("bin").join(cli_file_name()))
            .unwrap_or_else(|| PathBuf::from(cli_file_name()));

        emit(&app, "Done.");
        Ok(CliInstallResult {
            path: installed.display().to_string(),
            source: "source".into(),
            tracks_updates: false,
            message: "Built and installed from source. This copy is not linked to the \
                      app bundle, so app updates will not refresh it — re-run this \
                      after changing the CLI."
                .to_string(),
        })
    })
    .await
    .map_err(|e| format!("install task panicked: {e}"))?
}

/// Re-point an existing managed symlink at this build's bundled CLI.
///
/// Called once at startup. The symlink normally survives an update untouched —
/// the bundle path does not change — but it goes stale if the app is moved or
/// reinstalled somewhere else, and a dangling `alexandria` on `PATH` is worse
/// than none. Repairing it here means the updater keeps the CLI current
/// without the user revisiting the dialog.
///
/// Strictly a repair: it only ever rewrites a symlink the user already asked
/// for, never creates one. Best-effort — a read-only `/usr/local/bin` just
/// leaves the link alone.
pub fn refresh_link_if_installed() {
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;

        let dest = install_path();
        // Absent, or not ours: nothing to repair.
        let Some(current) = link_target(&dest) else {
            return;
        };
        let Some(bundled) = bundled_cli() else {
            return;
        };
        if current == bundled {
            return;
        }

        if std::fs::remove_file(&dest).is_ok() && symlink(&bundled, &dest).is_ok() {
            log::info!(
                "repointed {} at {} after an app update",
                dest.display(),
                bundled.display()
            );
        } else {
            log::warn!(
                "{} points at {} which is not this build; could not repoint it",
                dest.display(),
                current.display()
            );
        }
    }
}

// ---- Uninstall ----------------------------------------------------------

#[tauri::command]
pub async fn cli_uninstall() -> Result<String, String> {
    let dest = install_path();
    if dest.symlink_metadata().is_err() {
        return Ok(format!("Nothing installed at {}", dest.display()));
    }
    // Only remove what we created. A regular file there is someone else's.
    if link_target(&dest).is_none() {
        return Err(format!(
            "{} is not a symlink created by this app — leaving it alone.",
            dest.display()
        ));
    }
    std::fs::remove_file(&dest).map_err(|e| format!("Could not remove {}: {e}", dest.display()))?;
    Ok(format!("Removed {}", dest.display()))
}

fn emit(app: &AppHandle, line: &str) {
    let _ = app.emit(PROGRESS_EVENT, line.to_string());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_file_name_matches_the_platform() {
        let name = cli_file_name();
        if cfg!(windows) {
            assert_eq!(name, "alexandria.exe");
        } else {
            assert_eq!(name, "alexandria");
        }
    }

    #[test]
    fn the_sidecar_name_is_distinct_from_the_app_binary() {
        // Both live in Contents/MacOS/ on macOS, and that directory is on a
        // case-insensitive filesystem by default — so the two names must
        // differ by more than case. The app executable is named after the
        // cargo binary, `alexandria-node`.
        assert_ne!(BUNDLED_NAME.to_lowercase(), "alexandria-node");
        // The installed command keeps the short name regardless.
        assert_eq!(CLI_NAME, "alexandria");
    }

    #[cfg(unix)]
    #[test]
    fn install_path_ends_in_the_command_name() {
        // The directory depends on what is writable on this machine, but the
        // filename is always the command users type.
        let path = install_path();
        assert_eq!(path.file_name().unwrap(), "alexandria");
        assert!(path.is_absolute(), "got {}", path.display());
    }

    #[cfg(unix)]
    #[test]
    fn is_writable_is_false_for_a_root_owned_dir() {
        // /usr is root-owned on every supported unix; if this ever passes as
        // writable the probe is broken and install would silently target a
        // directory it cannot use.
        assert!(!is_writable(Path::new("/usr")));
    }

    #[cfg(unix)]
    #[test]
    fn is_writable_is_true_for_a_temp_dir() {
        assert!(is_writable(&std::env::temp_dir()));
    }

    #[test]
    fn link_target_is_none_for_a_regular_file() {
        // The install path guards on this: a regular file at the managed
        // location belongs to something else and must never be replaced.
        let dir = std::env::temp_dir().join("alexandria-cli-install-test");
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("regular");
        std::fs::write(&file, b"not a link").expect("write fixture");
        assert!(link_target(&file).is_none());
        let _ = std::fs::remove_file(&file);
    }

    #[cfg(unix)]
    #[test]
    fn link_target_reports_the_target_even_when_it_is_missing() {
        use std::os::unix::fs::symlink;
        let dir = std::env::temp_dir().join("alexandria-cli-install-test");
        let _ = std::fs::create_dir_all(&dir);
        let link = dir.join("dangling");
        let _ = std::fs::remove_file(&link);
        symlink(dir.join("does-not-exist"), &link).expect("symlink fixture");

        // `canonicalize` would fail here; `read_link` still reports the aim,
        // which is what the status view needs to show.
        assert_eq!(link_target(&link), Some(dir.join("does-not-exist")));
        let _ = std::fs::remove_file(&link);
    }
}
