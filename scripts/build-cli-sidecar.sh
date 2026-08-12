#!/usr/bin/env bash
#
# Build the `alexandria` CLI and stage it where Tauri expects an externalBin
# sidecar: src-tauri/binaries/alexandria-cli-<target-triple>.
#
# The desktop release configs (tauri.desktop.macos/linux.conf.json) declare
# `bundle.externalBin: ["binaries/alexandria-cli"]`, so the release bundle ships
# the CLI beside the app binary. The app's "Install CLI" action symlinks that
# onto PATH, which is what lets the auto-updater refresh the CLI: an update
# replaces the bundle the link points into.
#
# Deliberately NOT wired into the base tauri.conf.json. A missing externalBin
# is a hard build error, so declaring it there would force a full CLI release
# build on every `tauri dev` run.
#
# Staged as `alexandria-cli-<triple>`. On macOS the sidecar lands in
# Contents/MacOS/ beside the app executable, which Tauri names after the cargo
# binary (`alexandria-node`); the distinct name keeps that directory readable.
# The command installed on PATH is still `alexandria`.
#
# Usage:
#   scripts/build-cli-sidecar.sh                      # host target
#   scripts/build-cli-sidecar.sh aarch64-apple-darwin # explicit target
set -euo pipefail

cd "$(dirname "$0")/.."

TARGET="${1:-}"
if [ -z "$TARGET" ]; then
  # `rustc -vV` is the authoritative host triple; `uname` guesses wrong on
  # cross-configured machines.
  TARGET="$(rustc -vV | awk '/^host: / { print $2 }')"
fi

EXT=""
case "$TARGET" in
  *windows*) EXT=".exe" ;;
esac

DEST_DIR="src-tauri/binaries"
DEST="$DEST_DIR/alexandria-cli-$TARGET$EXT"

echo "==> Building alexandria CLI for $TARGET"
cargo build --release --locked -p alexandria --target "$TARGET"

mkdir -p "$DEST_DIR"
cp "target/$TARGET/release/alexandria$EXT" "$DEST"
chmod +x "$DEST"

echo "==> Staged $DEST"
