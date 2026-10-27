# Releasing Alexandria

Status: live
Updated: 2026-10-27

How the CI/CD workflows are laid out, how to cut a release, and the secrets
they need.

## Workflows

| Workflow | File | Trigger | Publishes |
| :--- | :--- | :--- | :--- |
| CI | [`ci.yml`](../.github/workflows/ci.yml) | every push and pull request | no |
| Validate (Desktop) | [`validate-desktop.yml`](../.github/workflows/validate-desktop.yml) | manual, with an immutable tag | no (artifacts on the run) |
| Validate (Mobile) | [`validate-mobile.yml`](../.github/workflows/validate-mobile.yml) | manual, with an immutable tag | no (artifacts on the run) |
| Release (Desktop) | [`release-desktop.yml`](../.github/workflows/release-desktop.yml) | push of a `v*` tag, or manual | GitHub release + updater metadata |
| Release (Mobile) | [`release-mobile.yml`](../.github/workflows/release-mobile.yml) | push of a `v*` tag, or manual | GitHub release, TestFlight, Google Play internal track |

The validate and release entrypoints are thin wrappers over two reusable
workflows, [`desktop-shared.yml`](../.github/workflows/desktop-shared.yml) and
[`mobile-shared.yml`](../.github/workflows/mobile-shared.yml).

### Dispatch defaults

| Workflow | Default platforms | Notes |
| :--- | :--- | :--- |
| Validate (Desktop) | macOS, Linux x86_64, Windows | Linux ARM64 is opt-in. `dev_seeding` is off by default |
| Validate (Mobile) | Android | iOS is opt-in so the macOS runner is only used intentionally. iOS export defaults to `debugging`. Android uses a temporary CI keystore |
| Release (Desktop) | macOS, Linux x86_64, Windows | Linux ARM64 is opt-in. Updater artifacts are signed |
| Release (Mobile) | Android, iOS | iOS export defaults to `app-store-connect` |

A tag push runs both release workflows with these defaults. Dispatch a workflow
by hand from the **Actions** tab to override them or to re-run against an
existing tag.

## Cutting a release

1. Make sure the `Release Readiness` CI job is green (see below).
2. Create and push a tag. **It must start with `v`**; that prefix is what the
   release workflows trigger on, so a tag without it publishes nothing.

   ```bash
   git tag v0.0.1-alpha.2
   git push origin v0.0.1-alpha.2
   ```

3. `Release (Desktop)` and `Release (Mobile)` start automatically.

To validate a build without publishing:

1. Push the commit and an immutable tag for it. Use a tag that does **not**
   start with `v` (for example `0.6.1-dev-seeds.20261003.1`) so the release
   workflows do not fire.
2. Run `Validate (Desktop)` and/or `Validate (Mobile)` with that tag.

Validation builds may set `dev_seeding=true` to enable **Settings → Developer →
Test data** (see [demo setup](demo/setup.md#development-test-data-picker)).
Release workflows never enable `dev-seeding`. Validation artifacts are attached
to the workflow run and are never uploaded to the app stores.

### Versioning

The shared workflows set the app version from the tag before building, so a
tag `0.0.1-alpha` produces `0.0.1-alpha` bundle metadata and artifact names
rather than the in-repo development version.

iOS overlays Apple's numeric version fields: the marketing version is the tag's
`major.minor.patch`, and `CFBundleVersion` is the workflow run number plus
attempt (for example `317.2`). Multi-part prerelease tags such as
`0.7.0-alpha.seedfix.20261022.7` therefore stay out of Apple's numeric fields
while the full tag remains in Cargo/package metadata and artifact names. The
`ios-version` config mode is covered by the release-workflow regression tests
in CI. Validation and release workflows have separate run-number sequences.

### Expected artifacts

Artifacts are named `Alexandria-<version>-<platform>.<ext>`:

| Platform | Artifact |
| :--- | :--- |
| macOS | `Alexandria-<version>-macOS-Apple-Silicon.dmg` |
| Linux x86_64 | `Alexandria-<version>-Linux-x86_64.AppImage`, `.deb` |
| Linux ARM64 | `Alexandria-<version>-Linux-ARM64.AppImage`, `.deb` |
| Windows | `Alexandria-<version>-Windows-x64-Setup.exe` |
| iOS | `Alexandria-<version>-iOS.ipa` |
| Android | `Alexandria-<version>-Android.apk`, `.aab` |

Signed artifacts also produce `.sig` files. The desktop `finalize` job
generates and uploads `latest.json` for the auto-updater.

## Gates

### Release Readiness

Releases target macOS, Linux, Windows, Android and iOS. The `Release Readiness`
job in CI ([`scripts/check-release-readiness.mjs`](../scripts/check-release-readiness.mjs))
fails unless:

- the version matches across `package.json`, `package-lock.json`,
  `src-tauri/Cargo.toml` and `src-tauri/tauri.conf.json`;
- the updater public key is set (not `PLACEHOLDER_PUBKEY`);
- Android no longer routes tutoring through the stubs, and its build config
  enables `tutoring-video-android`;
- Windows uses `tutoring-video` without `tutoring-video-aec`;
- `Release (Mobile)` includes iOS and `Release (Desktop)` includes macOS by
  default.

### Desktop regression gates

Frontend changes run the Chromium/WebKit UI regression suite in CI. Desktop
packaging also runs a Windows startup smoke test against the extracted NSIS
payload before uploading artifacts: it requires successful network-profile
validation, completed app setup, a visible window, and no native menu bar.
Hash-verified resources and built-in plugins must keep their exact bytes on
Windows checkouts; the `.gitattributes` regression checks this with
`core.autocrlf`. The Mac validation job launches the packaged app and checks
that it survives startup. See [Desktop UI validation](desktop-ui-validation.md)
for commands, coverage limits, the native VM checklist, and the Omarchy
AppImage workaround.

### Cost controls

- CI uses change detection to skip Rust work for frontend-only changes and
  frontend work for backend-only changes.
- The security audit job only runs when Cargo, Rust, patch or workflow files
  changed.
- Publish workflows require an immutable tag, so validation and publishing do
  not compete.
- Every workflow sets `concurrency.cancel-in-progress`, so stale runs are
  cancelled.

## Setup

### Prerequisites

- An Apple Developer Program membership for macOS and iOS signing. The App
  Store Connect API key used for iOS must hold the **App Manager** role so CI
  can mint provisioning profiles.
- Administrator access to the GitHub repository.
- Rust and Cargo locally, to generate the updater signing key.

`iroh-live`, `iroh-moq` and `moq-media` are vendored under `crates/` as
path-referenced workspace members, so no private repository, git dependency,
`[patch]` entry or `CROSS_REPO_PAT` secret is needed.

### GitHub secrets

Set these under **Settings → Secrets and variables → Actions**.

| Secret | Description | How to obtain |
| :--- | :--- | :--- |
| `APPLE_MAC_CERTIFICATE` | macOS **Developer ID Application** certificate (.p12) | Export from Keychain Access; `base64 -i cert.p12 \| pbcopy` |
| `APPLE_MAC_CERTIFICATE_PASSWORD` | Password for the macOS .p12 | Set when exporting |
| `APPLE_MAC_SIGNING_IDENTITY` | macOS certificate name | Keychain Access, e.g. "Developer ID Application: Name (TEAMID)" |
| `APPLE_IOS_CERTIFICATE` | iOS **Apple Distribution** certificate (.p12) | Export from Keychain Access; `base64 -i ios_cert.p12 \| pbcopy`. Required for signed iOS exports |
| `APPLE_IOS_CERTIFICATE_PASSWORD` | Password for the iOS .p12 | Set when exporting |
| `APPLE_KEYCHAIN_PASSWORD` | Temporary keychain password on the runner | Any string |
| `APPLE_TEAM_ID` | Apple Developer Team ID | Apple Developer account → Membership. Optional; the mobile workflow falls back to a built-in default |
| `APPLE_API_ISSUER_ID` | App Store Connect API issuer ID | App Store Connect → Users and Access → Integrations → App Store Connect API |
| `APPLE_API_KEY_ID` | App Store Connect API key ID | Same page (e.g. "ABC123DEF4") |
| `APPLE_API_KEY_P8` | Raw contents of the `.p8` key, including the BEGIN/END lines | Download from App Store Connect |
| `TAURI_SIGNING_PRIVATE_KEY` | Tauri updater private key | `cargo tauri signer generate` |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | Password for that key | Set when generating |
| `ANDROID_KEYSTORE` | Android signing keystore (.jks) | `keytool`; `base64 -i release.jks \| pbcopy` |
| `ANDROID_KEYSTORE_PASSWORD` | Keystore password | Set when generating |
| `ANDROID_KEY_ALIAS` | Key alias in the keystore | Set when generating (e.g. "release-key") |
| `WINDOWS_CERTIFICATE` | Windows code-signing certificate (.pfx/.p12) | Base64 encode the file. Signs the Windows installer |
| `WINDOWS_CERTIFICATE_PASSWORD` | Password for it | Set when exporting |
| `GOOGLE_PLAY_SERVICE_ACCOUNT_JSON` | Google Play service account key (optional) | Google Cloud Console → IAM → Service Accounts; grant "Release Manager" in Play Console → API access |

### Auto-updater

1. Generate keys: `cargo tauri signer generate -w ~/.tauri-signer`.
2. Put the public key in `src-tauri/tauri.conf.json` under
   `plugins.updater.pubkey`.
3. Store the private key and its password as `TAURI_SIGNING_PRIVATE_KEY` and
   `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.
4. Check the updater endpoint in `tauri.conf.json` is
   `https://github.com/ifftu-dev/alexandria/releases/latest/download/latest.json`.

### iOS code signing (manual)

iOS uses **manual** signing. Automatic signing on CI falls back to Apple's
cloud signing service, which mints a placeholder `Apple Distribution: Tauri
(unset)` certificate and cannot use a locally installed profile. Before
`tauri ios build`, `mobile-shared.yml`:

1. **Probes App Store Connect credentials**, so a bad key, an unregistered App
   ID or a missing profile fails in about a minute rather than 27 minutes into
   the archive.
2. **Ensures an App Store provisioning profile**: enables the
   `associated-domains` capability (needed for universal links) on the App ID,
   then mints and installs an ACTIVE distribution profile named `Alexandria iOS
   App Store (CI)`. It reuses an existing ACTIVE profile and only ever deletes
   that CI-owned name, so hand-made profiles are untouched.
3. **Forces manual code signing** by patching the generated Xcode project to
   `CODE_SIGN_STYLE = Manual`, bound to that profile.

The build step gets `APPLE_IOS_CERTIFICATE`, `APPLE_IOS_CERTIFICATE_PASSWORD`,
`IOS_MOBILE_PROVISION` (the minted profile) and `APPLE_DEVELOPMENT_TEAM`. The
`APPLE_API_*` secrets are deliberately withheld from it, because supplying them
would re-select cloud signing.

### Store uploads

- **TestFlight (iOS):** uploaded with `xcrun altool` when `ios_export_method`
  is `app-store-connect`. The build appears after Apple's processing (5–15
  minutes). Needs `APPLE_API_KEY_ID`, `APPLE_API_ISSUER_ID` and
  `APPLE_API_KEY_P8`.
- **Google Play (Android):** the AAB goes to the **internal testing** track via
  `r0adkll/upload-google-play`, only when `GOOGLE_PLAY_SERVICE_ACCOUNT_JSON` is
  set and the build used the production keystore (`secret` signing mode).
  Promote to other tracks in the Play Console.

## Troubleshooting

| Symptom | Check |
| :--- | :--- |
| Notarization fails | The Apple API key needs the "App Manager" or "Admin" role. The workflow log shows `notarytool`'s error code |
| Linux build: "No space left on device" | The Linux jobs free disk space and move `RUSTUP_HOME`, `CARGO_HOME`, `CARGO_TARGET_DIR` and `TMPDIR` to `/mnt` (see `desktop-shared.yml`). Check whether the runner's disk layout or `/mnt` size changed |
| Linux ARM64 job never starts | It runs on `ubuntu-22.04-arm`, which GitHub's free tier offers only to public repositories. A private fork needs a self-hosted or paid runner |
| "unresolved import" in `src/tutoring/manager_mobile.rs` on a mobile check | `crates/iroh-live` re-exports the in-tree `moq-media` crate as `iroh_live::media`, feature-gated behind `video` (desktop/Android, ffmpeg) and `video-ios` (VideoToolbox). Pass the matching feature, as CI does (below) |

```bash
cargo check -p alexandria-node --target aarch64-apple-ios --features tutoring-video-ios
cargo check -p alexandria-node --target aarch64-linux-android --features tutoring-video-android
```
