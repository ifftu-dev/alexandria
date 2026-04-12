//! Android NDK environment setup for cross-compilation.
//!
//! Detects the NDK, builds the set of env vars that CMake, `cc-rs`, and
//! Cargo need to cross-compile Rust + C/C++ dependencies (opus, openssl,
//! etc.) for Android targets. Mirrors `.github/workflows/mobile-shared.yml`
//! lines 469–501.
//!
//! Without this, `alex build android` / `alex run android` fails with
//! `aarch64-linux-android-ranlib: command not found` and similar errors.

use anyhow::{bail, Context, Result};
use std::os::unix::fs as unix_fs;
use std::path::{Path, PathBuf};

const DEFAULT_API_LEVEL: u32 = 28;

/// Android cross-compilation environment.
pub struct AndroidEnv {
    pub ndk_home: PathBuf,
    pub sysroot: PathBuf,
    /// `<ndk>/toolchains/llvm/prebuilt/<host>/bin`
    pub toolchain_bin: PathBuf,
    /// Kept for diagnostics (logged on failure).
    #[allow(dead_code)]
    pub host_tag: &'static str,
    /// Cache directory with `aarch64-linux-android-{ar,ranlib,strip,nm}` symlinks.
    /// Prepended to PATH because some C deps (opus) call the unsuffixed names.
    pub shim_dir: PathBuf,
    pub api_level: u32,
}

impl AndroidEnv {
    /// Detect the NDK and set up the shim directory. Fails with a
    /// user-friendly error if the NDK cannot be found.
    pub fn detect() -> Result<Self> {
        let ndk_home = find_ndk().context(
            "Could not locate the Android NDK. Install it via Android Studio \
             (SDK Manager > SDK Tools > NDK) or set ANDROID_NDK_HOME / NDK_HOME \
             to the NDK root.",
        )?;

        let host_tag = host_tag()?;
        let toolchain_bin = ndk_home
            .join("toolchains/llvm/prebuilt")
            .join(host_tag)
            .join("bin");
        if !toolchain_bin.is_dir() {
            bail!(
                "NDK toolchain not found at {}. The NDK at {} may be incomplete.",
                toolchain_bin.display(),
                ndk_home.display()
            );
        }

        let sysroot = toolchain_bin
            .parent()
            .ok_or_else(|| anyhow::anyhow!("toolchain_bin has no parent"))?
            .join("sysroot");
        if !sysroot.is_dir() {
            bail!("NDK sysroot not found at {}", sysroot.display());
        }

        let api_level = std::env::var("ALEX_ANDROID_API")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_API_LEVEL);

        let shim_dir = shim_dir()?;
        populate_shim_dir(&shim_dir, &toolchain_bin)?;

        Ok(Self {
            ndk_home,
            sysroot,
            toolchain_bin,
            host_tag,
            shim_dir,
            api_level,
        })
    }

    /// Environment variables to layer on top of the parent environment
    /// before invoking `cargo tauri android …`.
    pub fn env_vars(&self) -> Vec<(String, String)> {
        let ndk_home = self.ndk_home.display().to_string();
        let sysroot = self.sysroot.display().to_string();
        let bin = self.toolchain_bin.display().to_string();
        let api = self.api_level;

        let aarch64_clang = format!("{bin}/aarch64-linux-android{api}-clang");
        let armv7_clang = format!("{bin}/armv7a-linux-androideabi{api}-clang");
        let x86_64_clang = format!("{bin}/x86_64-linux-android{api}-clang");
        let i686_clang = format!("{bin}/i686-linux-android{api}-clang");

        let llvm_ar = format!("{bin}/llvm-ar");
        let llvm_ranlib = format!("{bin}/llvm-ranlib");
        let llvm_strip = format!("{bin}/llvm-strip");
        let llvm_nm = format!("{bin}/llvm-nm");
        let cflags = format!("--sysroot={sysroot}");

        // PATH: prepend shim_dir + toolchain_bin to whatever the caller inherits.
        let current_path = std::env::var("PATH").unwrap_or_default();
        let new_path = format!("{}:{}:{}", self.shim_dir.display(), bin, current_path);

        let mut env: Vec<(String, String)> = vec![
            // Roots
            ("PATH".into(), new_path),
            ("ANDROID_NDK_HOME".into(), ndk_home.clone()),
            ("ANDROID_NDK_ROOT".into(), ndk_home.clone()),
            ("NDK_HOME".into(), ndk_home.clone()),
            ("CARGO_NDK_SYSROOT_PATH".into(), sysroot.clone()),
            ("SYSROOT".into(), sysroot.clone()),
            // Generic tools (used by cc-rs, CMake, and build scripts that
            // call bare `ar`/`ranlib` etc.)
            ("AR".into(), llvm_ar.clone()),
            ("RANLIB".into(), llvm_ranlib.clone()),
            ("STRIP".into(), llvm_strip.clone()),
            ("NM".into(), llvm_nm.clone()),
            ("ANDROID_CFLAGS".into(), cflags.clone()),
            ("TARGET_CFLAGS".into(), cflags.clone()),
            ("ANDROID_NDK_AR".into(), llvm_ar.clone()),
            ("ANDROID_NDK_RANLIB".into(), llvm_ranlib.clone()),
            ("ANDROID_NDK_STRIP".into(), llvm_strip.clone()),
        ];
        let _ = ndk_home; // used via clones above

        // Per-target entries for all four Android triples. Cargo accepts
        // both dash and underscore forms in env var names — set both to
        // be defensive.
        for (triple, clang) in [
            ("aarch64-linux-android", &aarch64_clang),
            ("armv7-linux-androideabi", &armv7_clang),
            ("x86_64-linux-android", &x86_64_clang),
            ("i686-linux-android", &i686_clang),
        ] {
            let triple_upper = triple.to_uppercase().replace('-', "_");
            env.push((format!("CARGO_TARGET_{triple_upper}_LINKER"), clang.clone()));
            env.push((format!("CARGO_TARGET_{triple_upper}_AR"), llvm_ar.clone()));
            env.push((
                format!("CARGO_TARGET_{triple_upper}_RANLIB"),
                llvm_ranlib.clone(),
            ));
            // cc-rs reads CC_<triple> / CFLAGS_<triple> in both dash and
            // underscore forms.
            let triple_underscore = triple.replace('-', "_");
            env.push((format!("CC_{triple}"), clang.clone()));
            env.push((format!("CC_{triple_underscore}"), clang.clone()));
            env.push((format!("CFLAGS_{triple}"), cflags.clone()));
            env.push((format!("CFLAGS_{triple_underscore}"), cflags.clone()));
            env.push((format!("NM_{triple}"), llvm_nm.clone()));
            env.push((format!("NM_{triple_underscore}"), llvm_nm.clone()));
        }

        // Primary target (arm64) — convenience names CMake and some
        // build scripts read directly.
        env.push(("ANDROID_CC".into(), aarch64_clang.clone()));
        env.push(("ANDROID_AARCH64_CC".into(), aarch64_clang.clone()));
        env.push(("ANDROID_NM".into(), llvm_nm.clone()));
        env.push(("ANDROID_AARCH64_NM".into(), llvm_nm.clone()));
        env.push(("TARGET_CC".into(), aarch64_clang));

        env
    }
}

// ── Detection ────────────────────────────────────────────────────────

fn find_ndk() -> Result<PathBuf> {
    // 1. Explicit env vars (first that points to an existing dir)
    for var in ["NDK_HOME", "ANDROID_NDK_HOME", "ANDROID_NDK_ROOT"] {
        if let Ok(val) = std::env::var(var) {
            let p = PathBuf::from(&val);
            if p.is_dir() {
                return Ok(p);
            }
        }
    }

    // 2. Highest version under $ANDROID_HOME/ndk or $ANDROID_SDK_ROOT/ndk
    for var in ["ANDROID_HOME", "ANDROID_SDK_ROOT"] {
        if let Ok(sdk) = std::env::var(var) {
            let ndk_root = PathBuf::from(&sdk).join("ndk");
            if let Some(latest) = latest_subdir(&ndk_root) {
                return Ok(latest);
            }
        }
    }

    bail!(
        "No Android NDK detected. Looked at NDK_HOME, ANDROID_NDK_HOME, \
         ANDROID_NDK_ROOT, $ANDROID_HOME/ndk/*, $ANDROID_SDK_ROOT/ndk/*."
    )
}

fn latest_subdir(root: &Path) -> Option<PathBuf> {
    let mut best: Option<PathBuf> = None;
    if let Ok(entries) = std::fs::read_dir(root) {
        for entry in entries.flatten() {
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                let p = entry.path();
                match &best {
                    None => best = Some(p),
                    Some(cur) => {
                        if p.file_name() > cur.file_name() {
                            best = Some(p);
                        }
                    }
                }
            }
        }
    }
    best
}

fn host_tag() -> Result<&'static str> {
    // NDK ships `darwin-x86_64` on macOS (Apple Silicon uses it via
    // Rosetta — same as CI). Linux/Windows have their own tag.
    if cfg!(target_os = "macos") {
        Ok("darwin-x86_64")
    } else if cfg!(target_os = "linux") {
        Ok("linux-x86_64")
    } else if cfg!(target_os = "windows") {
        Ok("windows-x86_64")
    } else {
        bail!("Android builds are not supported on this host OS")
    }
}

// ── Shim directory ───────────────────────────────────────────────────

fn shim_dir() -> Result<PathBuf> {
    let cache =
        dirs::cache_dir().ok_or_else(|| anyhow::anyhow!("could not determine user cache dir"))?;
    Ok(cache.join("alex/android-ndk-bin"))
}

/// Create the shim directory (if needed) with symlinks
/// `aarch64-linux-android-{ar,ranlib,strip,nm}` → `<bin>/llvm-<tool>` etc.
/// Idempotent: existing correct symlinks are left alone.
fn populate_shim_dir(shim: &Path, toolchain_bin: &Path) -> Result<()> {
    std::fs::create_dir_all(shim)
        .with_context(|| format!("failed to create shim dir {}", shim.display()))?;

    // (shim_name, llvm_tool)
    let entries = [
        ("ar", "llvm-ar"),
        ("ranlib", "llvm-ranlib"),
        ("strip", "llvm-strip"),
        ("nm", "llvm-nm"),
    ];

    // Shim four architecture-prefixed names.
    let prefixes = [
        "aarch64-linux-android",
        "armv7-linux-androideabi",
        "x86_64-linux-android",
        "i686-linux-android",
    ];

    for prefix in prefixes {
        for (suffix, tool) in entries {
            let link = shim.join(format!("{prefix}-{suffix}"));
            let target = toolchain_bin.join(tool);
            refresh_symlink(&link, &target)?;
        }
    }

    Ok(())
}

fn refresh_symlink(link: &Path, target: &Path) -> Result<()> {
    // If link already points at the right target, leave it alone.
    if let Ok(existing) = std::fs::read_link(link) {
        if existing == target {
            return Ok(());
        }
        std::fs::remove_file(link).ok();
    } else if link.exists() {
        std::fs::remove_file(link).ok();
    }
    unix_fs::symlink(target, link).with_context(|| {
        format!(
            "failed to symlink {} -> {}",
            link.display(),
            target.display()
        )
    })
}
