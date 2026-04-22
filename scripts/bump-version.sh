#!/usr/bin/env bash
# Usage: scripts/bump-version.sh <version>   e.g.  scripts/bump-version.sh 1.2.3
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$SCRIPT_DIR/.."

VERSION="${1:-}"
if [ -z "$VERSION" ]; then
  echo "Usage: $0 <version>  (e.g. 1.2.3 or v1.2.3)" >&2
  exit 1
fi
VERSION="${VERSION#v}"  # strip leading 'v' if present

echo "Bumping to $VERSION..."

# package.json
jq --arg v "$VERSION" '.version = $v' "$ROOT/package.json" \
  > "$ROOT/package.json.tmp" && mv "$ROOT/package.json.tmp" "$ROOT/package.json"
echo "  Updated package.json"

# package-lock.json
jq --arg v "$VERSION" '.version = $v | .packages[""].version = $v' "$ROOT/package-lock.json" \
  > "$ROOT/package-lock.json.tmp" && mv "$ROOT/package-lock.json.tmp" "$ROOT/package-lock.json"
echo "  Updated package-lock.json"

# src-tauri/tauri.conf.json
jq --arg v "$VERSION" '.version = $v' "$ROOT/src-tauri/tauri.conf.json" \
  > "$ROOT/src-tauri/tauri.conf.json.tmp" \
  && mv "$ROOT/src-tauri/tauri.conf.json.tmp" "$ROOT/src-tauri/tauri.conf.json"
echo "  Updated src-tauri/tauri.conf.json"

# src-tauri/Cargo.toml (first [package] version field only).
# awk is used in place of sed for portability — BSD sed (macOS) does not
# support GNU's `0,/regex/{...}` first-match syntax.
awk -v ver="$VERSION" '
  !done && /^version = "[^"]*"/ { sub(/"[^"]*"/, "\"" ver "\""); done = 1 }
  { print }
' "$ROOT/src-tauri/Cargo.toml" > "$ROOT/src-tauri/Cargo.toml.tmp" \
  && mv "$ROOT/src-tauri/Cargo.toml.tmp" "$ROOT/src-tauri/Cargo.toml"
echo "  Updated src-tauri/Cargo.toml"

# Cargo.lock — refresh the alexandria-node entry so subsequent builds
# don't re-pin the old version.
if command -v cargo >/dev/null 2>&1; then
  (cd "$ROOT" && cargo update -p alexandria-node >/dev/null 2>&1) \
    && echo "  Updated Cargo.lock" \
    || echo "  Skipped Cargo.lock (cargo update failed; run manually)"
else
  echo "  Skipped Cargo.lock (cargo not found; run 'cargo update -p alexandria-node')"
fi

echo ""
echo "Done. Version is now $VERSION."
echo ""
echo "Next steps:"
echo "  git add package.json package-lock.json src-tauri/tauri.conf.json src-tauri/Cargo.toml Cargo.lock"
echo "  git commit -m 'chore: bump version to $VERSION'"
echo "  git tag v$VERSION"
echo "  git push && git push --tags"
