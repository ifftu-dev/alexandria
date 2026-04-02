# Release Pipelines

The release system is split into three lanes:

- `CI`: fast verification on pushes and pull requests.
- `Validate (Desktop)` / `Validate (Mobile)`: manual tag-based artifact builds without publishing.
- `Release (Desktop)` / `Release (Mobile)`: manual publish workflows for real releases.

## Public release gate

The first public `0.0.1-alpha` release is gated on all targeted platforms:

- `macOS`
- `Linux`
- `Windows`
- `Android`
- `iOS`

Before publishing a public release, the repo must pass the cheap `Release Readiness` gate in CI. That check currently enforces:

- release metadata stays version-aligned across `package.json`, `src-tauri/Cargo.toml`, and `src-tauri/tauri.conf.json`
- desktop updater config is no longer using the placeholder public key
- Android is not still routed through tutoring stubs
- Android build config opts into its real mobile tutoring feature set
- Windows is not using the reduced desktop tutoring feature path while macOS/Linux use richer media support
- real mobile publish defaults include both `Android` and `iOS`

## Cost controls

- CI uses change detection to skip Rust work for frontend-only changes and skip frontend work for backend-only changes.
- Security audit only runs when Cargo, Rust, patch, or workflow files changed.
- Desktop validation lets you opt into `macOS` and `Linux ARM64` instead of burning those minutes by default.
- Mobile validation defaults `iOS` off so the expensive macOS runner is only used intentionally, and Android validation uses a temporary CI keystore so it can build release-style Android artifacts without production signing secrets.
- Publish workflows are manual-only and require an immutable tag, which keeps validation and publishing from competing with each other.
- All workflows keep `concurrency.cancel-in-progress` enabled so stale runs are canceled automatically.

## How to use it

For day-to-day validation:

1. Push the commit you want to validate.
2. Create and push an immutable tag for that commit.
3. Run `Validate (Desktop)` for desktop artifacts.
4. Run `Validate (Mobile)` for Android, and enable iOS only when you need it.

For real releases:

1. Push the release tag.
2. Run `Release (Desktop)` manually with that tag to publish desktop bundles and updater metadata.
3. Run `Release (Mobile)` manually with that tag to publish Android and iOS artifacts.

Release workflows now sync the app version from the immutable release tag before building, so a tag like `0.0.1-alpha` produces `0.0.1-alpha` bundle metadata and artifact names instead of reusing the in-repo development version.

## Workflow files

- [`.github/workflows/ci.yml`](/Users/hack/Documents/Personal/Code/alexandria-mark3/alexandria/.github/workflows/ci.yml)
- [`.github/workflows/validate-desktop.yml`](/Users/hack/Documents/Personal/Code/alexandria-mark3/alexandria/.github/workflows/validate-desktop.yml)
- [`.github/workflows/release-desktop.yml`](/Users/hack/Documents/Personal/Code/alexandria-mark3/alexandria/.github/workflows/release-desktop.yml)
- [`.github/workflows/validate-mobile.yml`](/Users/hack/Documents/Personal/Code/alexandria-mark3/alexandria/.github/workflows/validate-mobile.yml)
- [`.github/workflows/release-mobile.yml`](/Users/hack/Documents/Personal/Code/alexandria-mark3/alexandria/.github/workflows/release-mobile.yml)
