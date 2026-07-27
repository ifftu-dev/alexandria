#!/usr/bin/env bash
# Cargo target runner for aarch64-linux-android. Cargo invokes this with the
# freshly cross-compiled test binary as $1 (plus any test args in $@); we push
# it to a connected device/emulator over adb and run it there, forwarding the
# exit code so `cargo test` reflects the on-device result.
#
# Wired via CARGO_TARGET_AARCH64_LINUX_ANDROID_RUNNER. Requires an arm64 AVD
# already booted (see the mobile-grader-test CI job) so native code JIT-compiled
# by the grader executes on the ABI users actually run.
set -euo pipefail

bin="$1"; shift
dev_dir="/data/local/tmp/alex-test"
dev_bin="$dev_dir/$(basename "$bin")"

adb shell "mkdir -p $dev_dir"
adb push "$bin" "$dev_bin" >/dev/null
adb shell "chmod 755 $dev_bin"

# TMPDIR: the wiring test writes a plugin bundle to a scratch dir; point it at
# device-writable storage. Forward remaining args (test filter, --exact, etc.).
adb shell "cd $dev_dir && TMPDIR=$dev_dir $dev_bin $*"
code=$?

adb shell "rm -rf $dev_dir" || true
exit $code
