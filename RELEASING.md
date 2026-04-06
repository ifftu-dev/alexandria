# Releasing Alexandria

This document explains how to set up the CI/CD pipeline and create releases.

## Prerequisites

To manage releases for Alexandria, you need:
- An Apple Developer Program membership for macOS and iOS signing.
- A GitHub account with administrator access to the repository.
- Rust and Cargo installed locally for generating signing keys.
- Access to the `iroh-live-patched` private repository.

## GitHub Secrets Setup

Configure these secrets in your GitHub repository settings under **Settings > Secrets and variables > Actions**.

| Secret Name | Description | How to Obtain |
| :--- | :--- | :--- |
| `CROSS_REPO_PAT` | GitHub Personal Access Token with `repo` scope. | Create in GitHub Developer Settings. Required to clone the private `iroh-live-patched` repository. |
| `APPLE_CERTIFICATE` | macOS/iOS signing certificate (.p12 file). | Export from Keychain Access on macOS. Base64 encode the file: `base64 -i cert.p12 \| pbcopy`. |
| `APPLE_CERTIFICATE_PASSWORD` | Password for the `.p12` certificate file. | Set this when exporting the certificate from Keychain Access. |
| `KEYCHAIN_PASSWORD` | Temporary keychain password for CI. | Any string. The CI uses this to create a temporary keychain on the runner. |
| `APPLE_SIGNING_IDENTITY` | Certificate name string. | Find in Keychain Access (e.g., "Developer ID Application: Name (TEAMID)"). |
| `APPLE_API_ISSUER` | App Store Connect API issuer ID. | Found in App Store Connect > Users and Access > Integrations > App Store Connect API. |
| `APPLE_API_KEY` | App Store Connect API key ID. | Found in the same App Store Connect API section (e.g., "ABC123DEF4"). |
| `APPLE_API_KEY_CONTENT` | Raw contents of the `.p8` API key file. | Download the `.p8` file from App Store Connect and copy its text content. |
| `TAURI_SIGNING_PRIVATE_KEY` | Tauri updater private key. | Generate using `cargo tauri signer generate`. |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | Password for the Tauri signer key. | Set this when generating the key. |
| `ANDROID_KEYSTORE` | Android signing keystore (.jks file). | Generate using `keytool`. Base64 encode the file: `base64 -i release.jks \| pbcopy`. |
| `ANDROID_KEYSTORE_PASSWORD` | Password for the Android keystore. | Set this when generating the keystore. |
| `ANDROID_KEY_ALIAS` | Key alias for the Android keystore. | Set this when generating the keystore (e.g., "release-key"). |

## Creating a Release

The CI/CD pipeline automates the build and release process.

### Tag-based Releases
Release builds are manual and consume an immutable tag:
1. Create a tag: `git tag 0.0.1-alpha.2`.
2. Push the tag: `git push origin 0.0.1-alpha.2`.
3. Run `Release (Desktop)` and `Release (Mobile)` from the **Actions** tab using that tag.

The workflows build artifacts for macOS, Linux (x86_64 and ARM64), Windows, iOS, and Android.

### Manual Triggers
Manual triggers are available via the **Actions** tab in GitHub:
1. Select the **Release (Desktop)** or **Release (Mobile)** workflow.
2. Click **Run workflow**.
3. Provide the immutable tag name you want to build.

### Expected Artifacts

Release artifacts follow the naming convention `Alexandria-<version>-<platform>.<ext>`:

| Platform | Artifact |
| :--- | :--- |
| macOS | `Alexandria-<version>-macOS-Apple-Silicon.dmg` |
| Linux x86_64 | `Alexandria-<version>-Linux-x86_64.AppImage`, `.deb` |
| Linux ARM64 | `Alexandria-<version>-Linux-ARM64.AppImage`, `.deb` |
| Windows | `Alexandria-<version>-Windows-x64-Setup.exe` |
| iOS | `Alexandria-<version>-iOS.ipa` |
| Android | `Alexandria-<version>-Android.apk`, `.aab` |

Signed artifacts also produce `.sig` files. The desktop finalize job generates `latest.json` for the auto-updater.

## Auto-Updater Setup

Alexandria uses the Tauri updater to deliver seamless updates.

1. **Generate Keys**: Run `cargo tauri signer generate -w ~/.tauri-signer`.
2. **Configure Public Key**: Copy the public key into `src-tauri/tauri.conf.json` under `plugins.updater.pubkey`.
3. **Store Private Key**: Save the private key and its password as GitHub secrets (`TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`).
4. **Check Endpoints**: Verify that the updater endpoint in `tauri.conf.json` points to your repository's latest release: `https://github.com/ifftu-dev/alexandria/releases/latest/download/latest.json`.

The `finalize` job in the desktop workflow automatically generates and uploads `latest.json` to the GitHub release.

The dispatch entrypoints are thin wrappers over the shared `desktop-shared.yml` and `mobile-shared.yml` reusable workflows.

## Troubleshooting

### Notarization Failures
Verify your Apple API Key has the "App Manager" or "Admin" role. Check the workflow logs for specific error codes from Apple's `notarytool`.

### Linux Build Space
The Linux builds use `maximize-build-space` to ensure enough room for the heavy Rust compilation. If builds fail with "No space left on device", check if the runner has changed its default disk layout.

### ARM64 Linux Availability
ARM64 builds for Linux run on `ubuntu-22.04-arm` runners. These are only available for public repositories on GitHub's free tier. If your repository is private, you must use a self-hosted runner or a paid GitHub runner plan.

### iroh-live-patched Repository
The workflows assume the patched library is at `ifftu-dev/iroh-live`. If your repository uses a different slug, update the `repository` field in the checkout steps of all workflow files.
