#!/usr/bin/env bash
# Cargo target runner for Android. Cargo invokes this with the freshly
# cross-compiled test binary as $1 (plus any test args in $@); we push it to a
# connected device/emulator over adb and run it there, forwarding the exit code
# so `cargo test` reflects the on-device result.
#
# ABI-agnostic: wired via CARGO_TARGET_<ABI>_LINUX_ANDROID_RUNNER, and nothing
# below depends on the architecture. CI uses x86_64 — see the mobile-grader-test
# job for why an arm64 AVD is not available there — but pointing an arm64 target
# at this script with a real device attached works unchanged.
set -euo pipefail

bin="$1"; shift
dev_dir="/data/local/tmp/alex-test"
dev_bin="$dev_dir/$(basename "$bin")"

adb shell "mkdir -p $dev_dir"
adb push "$bin" "$dev_bin" >/dev/null
adb shell "chmod 755 $dev_bin"

# TMPDIR: the wiring test writes a plugin bundle to a scratch dir; point it at
# device-writable storage. Forward remaining args (test filter, --exact, etc.).
#
# `|| code=$?` rather than a bare call followed by `code=$?`: under `set -e` a
# failing test aborts the script at the adb line, so the assignment on the next
# line only ever ran after a *passing* test — meaning the cleanup below was
# skipped for exactly the runs that leave a binary behind on the device.
code=0
adb shell "cd $dev_dir && TMPDIR=$dev_dir $dev_bin $*" || code=$?

adb shell "rm -rf $dev_dir" || true
exit "$code"
