# Release Pipelines

The release system is split into three lanes:

- `CI`: fast verification on pushes and pull requests.
- `Validate (Desktop)` / `Validate (Mobile)`: manual tag-based artifact builds without publishing.
- `Release (Desktop)` / `Release (Mobile)`: manual publish workflows for real releases.

## Cost controls

- CI uses change detection to skip Rust work for frontend-only changes and skip frontend work for backend-only changes.
- Security audit only runs when Cargo, Rust, patch, or workflow files changed.
- Desktop validation lets you opt into `macOS` and `Linux ARM64` instead of burning those minutes by default.
- Mobile validation defaults `iOS` off so the expensive macOS runner is only used intentionally.
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
3. Run `Release (Mobile)` manually with that tag to publish Android artifacts and, when enabled, signed iOS exports.

## Workflow files

- [`.github/workflows/ci.yml`](/Users/hack/Documents/Personal/Code/alexandria-mark3/alexandria/.github/workflows/ci.yml)
- [`.github/workflows/validate-desktop.yml`](/Users/hack/Documents/Personal/Code/alexandria-mark3/alexandria/.github/workflows/validate-desktop.yml)
- [`.github/workflows/release-desktop.yml`](/Users/hack/Documents/Personal/Code/alexandria-mark3/alexandria/.github/workflows/release-desktop.yml)
- [`.github/workflows/validate-mobile.yml`](/Users/hack/Documents/Personal/Code/alexandria-mark3/alexandria/.github/workflows/validate-mobile.yml)
- [`.github/workflows/release-mobile.yml`](/Users/hack/Documents/Personal/Code/alexandria-mark3/alexandria/.github/workflows/release-mobile.yml)
