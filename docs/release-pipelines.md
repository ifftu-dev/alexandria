# Release Pipelines

The release system is split into three lanes:

- `CI`: fast verification on pushes and pull requests.
- `Validate (Desktop)` / `Validate (Mobile)`: manual tag-based artifact builds without publishing.
- `Release (Desktop)` / `Release (Mobile)`: publish workflows for real releases. Pushing a `v*` tag fires them automatically with the dispatch defaults; `workflow_dispatch` stays available to override a run.

The validate/release entrypoints are now thin wrappers over two shared reusable workflows:

- `desktop-shared.yml`
- `mobile-shared.yml`

## Public release gate

The first public `0.0.1-alpha` release is gated on all targeted platforms:

- `macOS`
- `Linux`
- `Windows`
- `Android`
- `iOS`

Before publishing a public release, the repo must pass the cheap `Release Readiness` gate in CI. That check currently enforces:

- release metadata stays version-aligned across `package.json`, `package-lock.json`, `src-tauri/Cargo.toml`, and `src-tauri/tauri.conf.json`
- desktop updater config is no longer using the placeholder public key
- Android and iOS calls use their real media adapters (`PlatformMedia` in `src-tauri/src/tutoring/media/mod.rs`), never the synthetic test media
- Android build config opts into its real mobile tutoring feature set
- Windows uses the reduced desktop tutoring feature path while macOS/Linux use richer media support
- real mobile publish defaults include both `Android` and `iOS`

## Cost controls

- CI uses change detection to skip Rust work for frontend-only changes and skip frontend work for backend-only changes.
- Security audit only runs when Cargo, Rust, patch, or workflow files changed.
- Desktop validation runs macOS by default and lets you opt into `Linux ARM64`, which is off.
- Mobile validation defaults `iOS` off so the expensive macOS runner is only used intentionally, and Android validation uses a temporary CI keystore so it can build release-style Android artifacts without production signing secrets.
- Publish workflows require an immutable tag, which keeps validation and publishing from competing with each other. They trigger on a `v*` tag push and can also be dispatched by hand.
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

Shared desktop/mobile build workflows now sync the app version from the immutable release tag before building, so a tag like `0.0.1-alpha` produces `0.0.1-alpha` bundle metadata and artifact names instead of reusing the in-repo development version.

iOS uses a platform overlay for Apple's numeric version fields: the marketing
version is the tag's `major.minor.patch`, and `CFBundleVersion` is the GitHub workflow
run number plus attempt (for example, `317.2`). This keeps multi-part prerelease
tags such as `0.7.0-alpha.seedfix.20261022.7` out of Apple's numeric fields while
preserving the full tag in Cargo/package metadata and artifact names. The
`ios-version` config mode is covered by the release-workflow regression tests in
CI. Validation and release workflows have separate run-number sequences; validation
artifacts are not uploaded to TestFlight.

## Desktop regression gates

Frontend changes run the Chromium/WebKit UI regression suite in CI. Desktop
packaging also runs a Windows startup smoke test against the extracted NSIS
payload before uploading artifacts. It requires successful network-profile
validation, completed app setup, a visible window, and no native menu bar.
Hash-verified resources and built-in plugins must keep their exact bytes on
Windows checkouts; the `.gitattributes` regression checks this with `core.autocrlf`.
See [Desktop UI validation](desktop-ui-validation.md) for commands, coverage limits,
the native VM checklist, and the tested Omarchy AppImage workaround.

Validation builds may opt into `dev_seeding` for Settings → Developer → Test data.
Plugin grader compilation runs outside the database executor and reports progress
through the installing, compiling, and completed phases, keeping unrelated database
work responsive. Production releases leave this development-only feature disabled.

## Workflow files

- [`.github/workflows/ci.yml`](/Users/hack/Documents/Personal/Code/alexandria-mark3/alexandria/.github/workflows/ci.yml)
- [`.github/workflows/validate-desktop.yml`](/Users/hack/Documents/Personal/Code/alexandria-mark3/alexandria/.github/workflows/validate-desktop.yml)
- [`.github/workflows/release-desktop.yml`](/Users/hack/Documents/Personal/Code/alexandria-mark3/alexandria/.github/workflows/release-desktop.yml)
- [`.github/workflows/validate-mobile.yml`](/Users/hack/Documents/Personal/Code/alexandria-mark3/alexandria/.github/workflows/validate-mobile.yml)
- [`.github/workflows/release-mobile.yml`](/Users/hack/Documents/Personal/Code/alexandria-mark3/alexandria/.github/workflows/release-mobile.yml)
- [`.github/workflows/desktop-shared.yml`](/Users/hack/Documents/Personal/Code/alexandria-mark3/alexandria/.github/workflows/desktop-shared.yml)
- [`.github/workflows/mobile-shared.yml`](/Users/hack/Documents/Personal/Code/alexandria-mark3/alexandria/.github/workflows/mobile-shared.yml)
