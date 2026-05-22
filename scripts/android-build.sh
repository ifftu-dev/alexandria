#!/usr/bin/env bash
# Build the Android app, exporting the NDK cross-compile environment that
# `cargo tauri android build` does not set on its own. The app's native
# dependency chain (openssl-sys, gemm, audiopus_sys/opus, ffmpeg-sys-next)
# each needs a piece of the NDK toolchain wired up:
#
#   - NDK llvm bin on PATH        → openssl-sys vendored `ranlib`
#   - CMake toolchain + Ninja     → audiopus_sys (opus) C build
#   - CARGO_NDK_SYSROOT_PATH + CC → ffmpeg-sys-next cross build
#   - RUSTFLAGS=+fullfp16         → gemm-f16 fp16 NEON  (see CAVEAT below)
#
# Usage:
#   ./scripts/android-build.sh                 # debug, aarch64
#   ./scripts/android-build.sh --release        # extra args pass through
#
# Env overrides: ANDROID_HOME, NDK_HOME, ANDROID_API (default 28, matches
# tauri.android.conf.json minSdkVersion), ANDROID_TARGET (default aarch64).
#
# CAVEAT: `+fullfp16` is forced globally so gemm-f16 compiles — its fp16 NEON
# intrinsics (gemm-common simd.rs) lack `#[target_feature(enable="fp16")]`, so
# they only build under the global flag. Latest gemm (0.19) and candle (0.10)
# still have this defect, so a version bump does not help. The compiled fp16
# kernels can SIGILL on arm64 devices without ARMv8.2-FP16 — but the app's
# candle use is F32-only (DType::F32 in sentinel/{mouse_cnn,keystroke_ae}.rs),
# so those kernels are never executed. Before any f16 dtype is introduced,
# patch gemm-common to gate the intrinsics behind a runtime-detected
# target_feature fn (see `[patch.crates-io]` convention in patches/).
set -euo pipefail

cd "$(dirname "$0")/.."

ANDROID_HOME="${ANDROID_HOME:-$HOME/Library/Android/sdk}"
ANDROID_API="${ANDROID_API:-28}"
ANDROID_TARGET="${ANDROID_TARGET:-aarch64}"

# --- locate the NDK -------------------------------------------------------
NDK_HOME="${NDK_HOME:-${ANDROID_NDK_HOME:-}}"
if [ -z "$NDK_HOME" ]; then
    # newest installed NDK under $ANDROID_HOME/ndk
    NDK_HOME="$(find "$ANDROID_HOME/ndk" -maxdepth 1 -mindepth 1 -type d 2>/dev/null | sort -V | tail -1)"
fi
if [ -z "$NDK_HOME" ] || [ ! -d "$NDK_HOME" ]; then
    echo "error: NDK not found. Set NDK_HOME or install one under $ANDROID_HOME/ndk" >&2
    exit 1
fi

# --- host prebuilt tag (NDK uses *-x86_64 even on arm64 hosts) -------------
case "$(uname -s)" in
    Darwin) HOST_TAG="darwin-x86_64" ;;
    Linux)  HOST_TAG="linux-x86_64" ;;
    *) echo "error: unsupported host $(uname -s)" >&2; exit 1 ;;
esac

NDKBIN="$NDK_HOME/toolchains/llvm/prebuilt/$HOST_TAG/bin"
SYSROOT="$NDK_HOME/toolchains/llvm/prebuilt/$HOST_TAG/sysroot"
CC_PATH="$NDKBIN/${ANDROID_TARGET}-linux-android${ANDROID_API}-clang"
# armv7 uses the "armv7a-...androideabi" prefix; fix up if targeting it.
if [ "$ANDROID_TARGET" = "armv7" ]; then
    CC_PATH="$NDKBIN/armv7a-linux-androideabi${ANDROID_API}-clang"
fi

for p in "$NDKBIN" "$SYSROOT" "$CC_PATH" \
         "$NDK_HOME/build/cmake/android.toolchain.cmake"; do
    [ -e "$p" ] || { echo "error: missing NDK component: $p" >&2; exit 1; }
done

export PATH="$NDKBIN:$PATH"
export ANDROID_NDK_ROOT="$NDK_HOME" ANDROID_NDK_HOME="$NDK_HOME"
export CMAKE_TOOLCHAIN_FILE="$NDK_HOME/build/cmake/android.toolchain.cmake"
export CMAKE_GENERATOR="Ninja"
export CARGO_NDK_SYSROOT_PATH="$SYSROOT"
# ffmpeg-sys-next reads CC_<target_underscored>; the dashed name can't be
# `export`ed from zsh, so the underscored form is the portable choice.
export "CC_${ANDROID_TARGET}_linux_android=$CC_PATH"
export RUSTFLAGS="${RUSTFLAGS:-} -C target-feature=+fullfp16"

echo "NDK:     $NDK_HOME"
echo "target:  ${ANDROID_TARGET} (API $ANDROID_API)"
echo "CC:      $CC_PATH"
echo

exec cargo tauri android build --target "$ANDROID_TARGET" "$@"
