#!/usr/bin/env bash
# Flatten the iOS AppIcon set so every PNG is fully opaque (no alpha channel).
#
# App Store Connect / TestFlight silently drop an app icon that carries an
# alpha channel, showing a blank icon. `tauri icon` composites our
# transparent-background source (src-tauri/icons/icon.png) onto white but
# leaves an alpha channel behind, so the generated iOS icons still report
# hasAlpha=yes. Run this after regenerating icons (`tauri icon`) to strip it.
#
# Idempotent: re-running on already-opaque icons is a no-op. Requires
# ImageMagick (`magick`).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ICONSET="$SCRIPT_DIR/../src-tauri/gen/apple/Assets.xcassets/AppIcon.appiconset"

if ! command -v magick >/dev/null 2>&1; then
  echo "error: ImageMagick (magick) not found" >&2
  exit 1
fi

[ -d "$ICONSET" ] || { echo "error: iconset not found: $ICONSET" >&2; exit 1; }

changed=0
for f in "$ICONSET"/*.png; do
  magick "$f" -background white -alpha remove -alpha off "$f.tmp"
  mv "$f.tmp" "$f"
  changed=$((changed + 1))
done

echo "Flattened $changed iOS icon(s) to opaque (no alpha)."
