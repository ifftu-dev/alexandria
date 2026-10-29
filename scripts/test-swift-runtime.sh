#!/usr/bin/env bash
# Exercise the vendored Swift archive boundary on a macOS host. The vendored
# crate is not a workspace member, so test an isolated copy of its exact source.
set -euo pipefail
cd "$(dirname "$0")/.."
test "$(uname -s)" = Darwin
scratch=$(mktemp -d "${TMPDIR:-/tmp}/alexandria-swift-tests.XXXXXX")
trap 'rm -rf "$scratch"' EXIT
cp -R patches/swift-rs "$scratch/swift-rs"
# Match a clean CI checkout; local dependency crates can have ignored lockfiles.
rm -f "$scratch/swift-rs/Cargo.lock"
CARGO_TARGET_DIR="$scratch/target" cargo test \
  --manifest-path "$scratch/swift-rs/Cargo.toml" \
  --features build --lib runtime_export_tests
