# Building Alexandria

Status: live
Updated: 2026-10-27

System requirements, prerequisites, optional Cardano setup, and desktop
development builds. iOS and Android are in [building for mobile](building-mobile.md);
the test and lint gates are in [CONTRIBUTING](../CONTRIBUTING.md#before-you-push);
release builds are in [releasing](releasing.md).

## System requirements

Alexandria is a full peer-to-peer node, not a thin client. Every device runs
the P2P swarm (DHT, relay, username registry), the content store, an encrypted
database and, during live tutoring, real-time audio and video, so it needs more
than a typical viewer app.

### Desktop

| Platform | Minimum |
|----------|---------|
| macOS    | 10.15 Catalina or later (Intel or Apple Silicon) |
| Windows  | Windows 10+ with WebView2 (preinstalled on Windows 11) |
| Linux    | x86_64 with webkit2gtk 4.1 |

### iOS

- **iOS 16.4 or later** (the deployment target; the UI needs the WebKit that
  ships with 16.4).
- **Recommended:** iPhone SE (3rd gen), iPhone 13 or newer. Live tutoring uses
  the hardware H.264 encoder (VideoToolbox), so any supported iPhone handles
  video well.
- **Face ID / Touch ID** gives one-tap vault unlock; otherwise unlock each
  profile with its password.

### Android

- **Android 9 (API 28) or later** with Android System WebView (Chromium) 111+,
  which updates through Google Play.
- **Minimum hardware:** 4 GB RAM. The app runs, but live tutoring may stutter
  on entry-level chipsets.
- **Recommended:** 6–8 GB RAM and a mid-range 2023+ chipset (Snapdragon
  7-series, Dimensity 7300-class, Tensor G3 or better). Unlike iOS, Android
  encodes tutoring video in software (FFmpeg/x264), the most CPU-intensive
  thing the app does.
- **Fingerprint / face unlock** gives one-tap vault unlock; otherwise use the
  profile password.

### Storage and battery

- Plan for **2–4 GB free** beyond the app. Each profile keeps its own
  encrypted database, content-addressed blob store and video cache, which grow
  with downloaded course content.
- The P2P node keeps live network connections while the app is open. On
  phones, expect battery use comparable to a messaging app doing an active
  sync.

## Prerequisites

| Tool | Version | Notes |
|---|---|---|
| Rust + `cargo` | 1.91+ | iroh 1.0 sets the MSRV; see `rust-version` in `src-tauri/Cargo.toml` |
| Node.js + `npm` | 24.22.1 (see [`.nvmrc`](../.nvmrc)) | Also supported: 22.23.4+ within 22.x, 24.22.1+ within 24.x, 26.12.1+ within 26.x |
| Tauri CLI | 2.x | `cargo install tauri-cli` |

With nvm, run `nvm install` and `nvm use` from the repository root.
[`package.json`](../package.json) declares the supported Node versions and
[`.npmrc`](../.npmrc) makes `npm install` and `npm ci` reject anything outside
that range. These minimums include the CVE-2026-111497 Brotli decoder fix.

### Native packages for live audio/video tutoring

| Platform | Install |
|---|---|
| macOS | `brew install cmake ffmpeg libtool automake autoconf` |
| Linux | `sudo apt-get install -y cmake libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf nasm libclang-dev pkg-config libopus-dev libssl-dev libx264-dev libva-dev libgbm-dev libdrm-dev libvdpau-dev` |
| Windows | CMake; MSYS2 autotools (`pacman -S --noconfirm libtool automake autoconf`); a shared FFmpeg build with `FFMPEG_DIR` set and `FFMPEG_DIR\bin` on `PATH` |

### Cargo features

The `alexandria-node` crate has no default features, so a plain build has no
live video. Pick the feature for your target:

| Target | Feature |
|---|---|
| macOS, Windows | `tutoring-video` |
| Linux | `tutoring-video-static` (what CI uses) |
| iOS | `tutoring-video-ios` |
| Android | `tutoring-video-android` (opt-in shared video pipeline) |

## Desktop development

```bash
git clone git@github.com:ifftu-dev/alexandria.git
cd alexandria
npm ci
npm run dev                                   # terminal 1: Vite on http://localhost:5173
cargo tauri dev --features tutoring-video     # terminal 2 (Linux: tutoring-video-static)
```

`tauri.conf.json` has an empty `beforeDevCommand`, so `cargo tauri dev` does
not start Vite itself; keep `npm run dev` running. Drop `--features …` for a
faster build without live video.

The app opens a native window backed by a local webview. Every profile open
runs migrations, installs or refreshes the bundled taxonomy, goal templates
and question banks, and starts the iroh content store. This temporary demo
build also installs the demo courses, bundled assessments and plugins, and
classroom templates for each profile; it does not fabricate credentials or
enrolments. New and mnemonic-restored profiles pin the bundled
[default-network demo genesis](default-network-genesis.md); existing profiles
keep their trust decisions. See [demo setup](demo/setup.md).

To run a second, isolated instance beside your normal one (for example a
parent and a child profile pairing over LAN), set `ALEXANDRIA_DATA_DIR` to an
empty directory before launching. It is honoured on desktop only.

For a production desktop bundle, `cargo tauri build` runs `npm run build`
first. Signed, published builds come from CI; see [releasing](releasing.md).

## Cardano (optional)

On-chain features (NFT wrappers, governance transactions, soulbound and
reputation flows) need a Blockfrost project and test ADA. Without them the app
works fully offline and blockchain features are simply unavailable.

- **Blockfrost project id:** sign up at [blockfrost.io](https://blockfrost.io)
  and create a **preprod** project. Configure it either:
  - in the app (recommended): **Settings → Cardano → Blockfrost project id**.
    Stored per device and picked up without a restart; or
  - as an environment variable for CI and scripts:
    `export BLOCKFROST_PROJECT_ID="preprodXXX..."`.

  The in-app setting wins over the environment variable.
- **Test ADA:** fund your wallet address from the
  [Cardano faucet](https://docs.cardano.org/cardano-testnets/tools/faucet/):
  at least 5 tADA for wrapper/NFT minting, about 40 tADA to deploy governance
  validators.
- **Aiken** v1.1.21+ for smart-contract development:
  [aiken-lang.org](https://aiken-lang.org/installation-instructions).

Validators are deployed on preprod only. See
[`cardano/governance/README.md`](../cardano/governance/README.md).
