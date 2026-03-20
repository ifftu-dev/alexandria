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

# src-tauri/tauri.conf.json
jq --arg v "$VERSION" '.version = $v' "$ROOT/src-tauri/tauri.conf.json" \
  > "$ROOT/src-tauri/tauri.conf.json.tmp" \
  && mv "$ROOT/src-tauri/tauri.conf.json.tmp" "$ROOT/src-tauri/tauri.conf.json"
echo "  Updated src-tauri/tauri.conf.json"

# src-tauri/Cargo.toml (first [package] version field only)
sed -i.bak "0,/^version = \"[^\"]*\"/{s/^version = \"[^\"]*\"/version = \"$VERSION\"/}" \
  "$ROOT/src-tauri/Cargo.toml"
rm -f "$ROOT/src-tauri/Cargo.toml.bak"
echo "  Updated src-tauri/Cargo.toml"

echo ""
echo "Done. Version is now $VERSION."
echo ""
echo "Next steps:"
echo "  git add package.json src-tauri/tauri.conf.json src-tauri/Cargo.toml"
echo "  git commit -m 'chore: bump version to $VERSION'"
echo "  git tag v$VERSION"
echo "  git push && git push --tags"
