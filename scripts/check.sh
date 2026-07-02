#!/usr/bin/env bash
# Local mirror of CI's "Lint & Test" + "Security Audit" jobs — run before
# pushing to catch what CI would catch.
#
#   ./scripts/check.sh          # everything
#   ./scripts/check.sh --fast   # skip the Rust test suite (slowest step)
#
# One intentional divergence: CI lints/tests with the
# `tutoring-video-static` feature, which needs a static ffmpeg that does
# not build on macOS dev machines. Locally we use the default (dev)
# feature set — same code minus the static-ffmpeg linkage. Anything that
# only breaks under the static feature will still surface in CI.

set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
FAST=0
[[ "${1:-}" == "--fast" ]] && FAST=1

failures=()

step() {
  local name="$1"
  shift
  echo
  echo "──────────────────────────────────────────────"
  echo "▶ ${name}"
  echo "──────────────────────────────────────────────"
  if "$@"; then
    echo "✔ ${name}"
  else
    echo "✘ ${name}"
    failures+=("${name}")
  fi
}

cd "$ROOT/src-tauri"
step "cargo fmt --check" cargo fmt --check
step "cargo clippy -D warnings" cargo clippy -- -D warnings
if [[ $FAST -eq 0 ]]; then
  step "cargo test" cargo test
fi

cd "$ROOT"
step "vue-tsc type-check" npx vue-tsc -b --noEmit
step "frontend tests (vitest)" npm test

if command -v cargo-audit >/dev/null 2>&1; then
  step "cargo audit" cargo audit --file Cargo.lock
else
  echo
  echo "⚠ cargo-audit not installed — skipping Security Audit mirror."
  echo "  Install with: cargo install cargo-audit"
fi

echo
if [[ ${#failures[@]} -eq 0 ]]; then
  echo "All checks passed."
else
  echo "FAILED: ${failures[*]}"
  exit 1
fi
