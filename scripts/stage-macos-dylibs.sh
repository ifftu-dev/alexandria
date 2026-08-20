#!/usr/bin/env bash
#
# Make the macOS bundle self-contained.
#
# ffmpeg is linked statically out of Homebrew's .a files, but its own
# dependencies — libopus, libx264, libssl, libcrypto — come in as dylibs, and
# the linker records them by absolute path:
#
#     /opt/homebrew/opt/opus/lib/libopus.0.dylib
#
# There is no LC_RPATH entry to fall back on, so on any Mac without Homebrew
# and those exact formulae dyld cannot resolve them and the app does not
# launch. Every released DMG had this; it went unnoticed because the people
# building it all had Homebrew.
#
# This runs as `beforeBundleCommand`: after the binary is linked, before Tauri
# assembles the .app, so the fix is inside the DMG and the updater tarball
# rather than applied to one of them afterwards. It stages each dependency
# under a version-free name and points the binary at
# `@executable_path/../Frameworks/`; `bundle.macOS.frameworks` in
# tauri.desktop.macos.conf.json copies the staged files in.
#
# Names are normalised (libx264.165.dylib -> libx264.dylib) so that the config
# list does not have to be edited every time Homebrew bumps a soname. We
# reference exactly the copy we ship, so the version in the filename carries no
# meaning here.

set -euo pipefail

cd "$(dirname "$0")/.."

TARGET_TRIPLE="${1:-}"
# The cargo workspace root is this directory, so target/ lives here rather than
# under src-tauri/. Release builds pass `--target aarch64-apple-darwin`, which
# moves the binary under that triple; local builds do not. Tauri does not
# substitute anything into a hook command, so rather than take the triple as an
# argument we look in both places and take whichever was written most recently.
if [ -n "$TARGET_TRIPLE" ]; then
    BIN="target/$TARGET_TRIPLE/release/alexandria-node"
else
    BIN=""
    newest=0
    for candidate in target/release/alexandria-node target/*/release/alexandria-node; do
        [ -f "$candidate" ] || continue
        mtime=$(stat -f%m "$candidate" 2>/dev/null || echo 0)
        if [ "$mtime" -gt "$newest" ]; then newest="$mtime"; BIN="$candidate"; fi
    done
    BIN="${BIN:-target/release/alexandria-node}"
fi

STAGE="src-tauri/macos-frameworks"

if [ "$(uname -s)" != "Darwin" ]; then
    exit 0
fi

if [ ! -f "$BIN" ]; then
    echo "stage-macos-dylibs: no binary at $BIN — nothing to do" >&2
    exit 0
fi

rm -rf "$STAGE"
mkdir -p "$STAGE"

# Version-free name: libx264.165.dylib -> libx264.dylib
normalise() { basename "$1" | sed -E 's/\.[0-9]+(\.[0-9]+)*\.dylib$/.dylib/'; }

deps_of() { otool -L "$1" | tail -n +2 | grep -oE '/opt/homebrew[^ ]*\.dylib' || true; }

# Walk the graph: a staged library's own dependencies have to come along too,
# or we have simply moved the failure one level down (libssl needs libcrypto).
declare -a QUEUE=()
while IFS= read -r d; do [ -n "$d" ] && QUEUE+=("$d"); done < <(deps_of "$BIN")

declare -A SEEN=()
while [ ${#QUEUE[@]} -gt 0 ]; do
    src="${QUEUE[0]}"; QUEUE=("${QUEUE[@]:1}")
    [ -n "${SEEN[$src]:-}" ] && continue
    SEEN[$src]=1

    if [ ! -f "$src" ]; then
        echo "stage-macos-dylibs: $src is referenced but missing" >&2
        exit 1
    fi

    out="$(normalise "$src")"
    cp -f "$src" "$STAGE/$out"
    chmod u+w "$STAGE/$out"
    install_name_tool -id "@executable_path/../Frameworks/$out" "$STAGE/$out"

    while IFS= read -r sub; do [ -n "$sub" ] && QUEUE+=("$sub"); done < <(deps_of "$src")
done

if [ ${#SEEN[@]} -eq 0 ]; then
    echo "stage-macos-dylibs: binary has no Homebrew dependencies"
    exit 0
fi

# Repoint every reference — in the binary and between the staged libraries.
for src in "${!SEEN[@]}"; do
    out="$(normalise "$src")"
    install_name_tool -change "$src" "@executable_path/../Frameworks/$out" "$BIN" 2>/dev/null || true
    for staged in "$STAGE"/*.dylib; do
        install_name_tool -change "$src" "@executable_path/../Frameworks/$out" "$staged" 2>/dev/null || true
    done
done

# The linker's ad-hoc signature does not survive install_name_tool, and an
# unsigned Mach-O is fatal on arm64. Tauri re-signs the bundle properly later;
# this only has to be valid in the meantime.
codesign --force --sign - "$BIN" >/dev/null 2>&1 || true
for staged in "$STAGE"/*.dylib; do
    codesign --force --sign - "$staged" >/dev/null 2>&1 || true
done

remaining="$(deps_of "$BIN" | wc -l | tr -d ' ')"
if [ "$remaining" != "0" ]; then
    echo "stage-macos-dylibs: $remaining Homebrew reference(s) still in the binary" >&2
    deps_of "$BIN" >&2
    exit 1
fi

echo "stage-macos-dylibs: staged ${#SEEN[@]} librar(ies) into $STAGE"
ls "$STAGE" | sed 's/^/  /'
