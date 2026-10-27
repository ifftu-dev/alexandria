# Building for iOS and Android

Status: live
Updated: 2026-10-27

Device requirements and the shared prerequisites are in
[building](building.md#system-requirements). For a quicker loop with a device
picker, the developer CLI wraps both platforms: `alexandria run ios` and
`alexandria run android` (see [`cli/README.md`](../cli/README.md#running-on-mobile)).

When type-checking a mobile target, always pass the matching feature, as CI
does; without it you get misleading "unresolved import" errors in
`src/tutoring/manager_mobile.rs`:

```bash
cargo check -p alexandria-node --target aarch64-apple-ios-sim --features tutoring-video-ios
cargo check -p alexandria-node --target aarch64-linux-android --features tutoring-video-android
```

## iOS

### Prerequisites

- **Xcode 15+** with command-line tools.
- **Rust targets:** `rustup target add aarch64-apple-ios aarch64-apple-ios-sim`.
- **Apple Development Team** set in `src-tauri/tauri.conf.json`
  (`bundle.iOS.developmentTeam`).

### Build

```bash
cargo tauri ios build --target aarch64-sim   # simulator (Apple Silicon)
cargo tauri ios build --target aarch64       # physical device
```

### Install and launch

```bash
# Simulator
xcrun simctl install <SIMULATOR_UDID> src-tauri/gen/apple/build/arm64-sim/Alexandria.app
xcrun simctl launch <SIMULATOR_UDID> org.alexandria.node

# Device
xcrun devicectl device install app --device <DEVICE_UDID> src-tauri/gen/apple/build/arm64/Alexandria.ipa
xcrun devicectl device process launch --device <DEVICE_UDID> org.alexandria.node
```

Clean before rebuilding to avoid "Directory not empty" errors:

```bash
rm -rf src-tauri/gen/apple/build
```

## Android

### Prerequisites

- **Android Studio** with the SDK, NDK 26+ and Build Tools 34+.
- **Rust targets:**
  `rustup target add aarch64-linux-android armv7-linux-androideabi i686-linux-android x86_64-linux-android`.
- **Environment:** `ANDROID_HOME`, `NDK_HOME`, and `JAVA_HOME` (Android
  Studio's bundled JDK).
- **`ninja`** on `PATH`.

### Build

Use the wrapper script. A bare `cargo tauri android build` fails on the native
dependency chain, because Tauri does not export the NDK cross-compile
environment those crates need:

| Crate | Needs |
|---|---|
| `openssl-sys` (vendored, via SQLCipher) | `ANDROID_NDK_ROOT` **and** the NDK `llvm` bin on `PATH` (its Makefile calls `ranlib` by bare name) |
| `ffmpeg-sys-next` | `CARGO_NDK_SYSROOT_PATH` (normally set by `cargo ndk`, but Tauri drives `cargo build` directly) |
| `audiopus_sys` (Opus) | CMake + Ninja; aborts if a stale build dir used a different generator |

[`scripts/android-build.sh`](../scripts/android-build.sh) sets all of this up,
installs `cargo-ndk` if missing, clears the stale CMake cache, and forwards its
arguments to `cargo tauri android build`:

```bash
./scripts/android-build.sh --debug     # debug APK, for the emulator or sideloading
./scripts/android-build.sh --release   # release APK, unsigned: sign before distributing
```

Overrides: `ANDROID_HOME`, `NDK_HOME`, `ANDROID_API` (default 28, matching
`minSdkVersion`), `ANDROID_TARGET` (default `aarch64`).

### Install and launch

```bash
adb install src-tauri/gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk
adb shell am start -n org.alexandria.node/.MainActivity
```

Clean build artifacts:

```bash
rm -rf src-tauri/gen/android/app/build
```

### Safe-area insets

Android WebView does not support `env(safe-area-inset-*)`. `MainActivity.kt`
reads the real `WindowInsets` and injects them as the CSS custom properties
`--sat`, `--sab`, `--sal` and `--sar`.
