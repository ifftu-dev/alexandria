#!/usr/bin/env python3
"""Build the pinned synthetic ARM64 worker and stage an opt-in Android debug bundle."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
WORKER = ROOT / "tools/personhood-prover"


def digest(path):
    value = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            value.update(block)
    return value.hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ndk", required=True, type=Path)
    parser.add_argument("--artifacts", type=Path, default=WORKER / "artifacts")
    parser.add_argument("--output", type=Path, default=ROOT / "target/personhood-lab")
    parser.add_argument("--check", action="store_true", help="also run worker formatting and Android Clippy checks")
    args = parser.parse_args()
    artifacts = args.artifacts.resolve()
    artifacts.mkdir(parents=True, exist_ok=True)
    manifest = json.loads((WORKER / "artifact-manifest.json").read_text())
    for item in manifest:
        if item["name"] not in ("aadhaar-verifier.wasm", "vkey.json"):
            continue
        path = artifacts / item["name"]
        if not path.exists() or digest(path) != item["sha256"]:
            partial = path.with_suffix(path.suffix + ".partial")
            with urllib.request.urlopen(item["url"], timeout=60) as response, partial.open("wb") as output:
                copied = 0
                while block := response.read(1024 * 1024):
                    copied += len(block)
                    if copied > item["bytes"]:
                        raise ValueError("artifact exceeds pinned size")
                    output.write(block)
            if partial.stat().st_size != item["bytes"] or digest(partial) != item["sha256"]:
                partial.unlink()
                raise ValueError("artifact checksum mismatch")
            partial.replace(path)
    hosts = [path for path in (args.ndk.resolve() / "toolchains/llvm/prebuilt").glob("*")
             if (path / "bin/aarch64-linux-android28-clang").is_file()]
    if len(hosts) != 1:
        raise ValueError("expected one NDK host toolchain")
    host = hosts[0]
    env = os.environ.copy()
    for name in ("RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "CMAKE_TOOLCHAIN_FILE", "SDKROOT",
                 "IPHONEOS_DEPLOYMENT_TARGET", "RAPIDSNARK_LIB_DIR"):
        env.pop(name, None)
    env.update({
        "CARGO_ENCODED_RUSTFLAGS": "",
        "CARGO_TARGET_DIR": str(WORKER / "target"),
        "PERSONHOOD_CIRCUIT_DIR": str(artifacts),
        "CC_aarch64_linux_android": str(host / "bin/aarch64-linux-android28-clang"),
        "CXX_aarch64_linux_android": str(host / "bin/aarch64-linux-android28-clang++"),
        "AR_aarch64_linux_android": str(host / "bin/llvm-ar"),
        "CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER": str(host / "bin/aarch64-linux-android28-clang"),
    })
    subprocess.run(["cargo", "build", "--release", "--locked", "--target", "aarch64-linux-android", "-j", "6"],
                   cwd=WORKER, env=env, check=True)
    if args.check:
        subprocess.run(["cargo", "fmt", "--check"], cwd=WORKER, env=env, check=True)
        subprocess.run(["cargo", "clippy", "--release", "--locked", "--target", "aarch64-linux-android",
                        "--", "-D", "warnings"], cwd=WORKER, env=env, check=True)
    output = args.output.resolve()
    libraries = output / "jniLibs/arm64-v8a"
    assets = output / "assets/personhood-lab"
    libraries.mkdir(parents=True, exist_ok=True)
    assets.mkdir(parents=True, exist_ok=True)
    shutil.copy2(WORKER / "target/aarch64-linux-android/release/personhood-native-bench", libraries / "libpersonhood_bench.so")
    shutil.copy2(host / "sysroot/usr/lib/aarch64-linux-android/libc++_shared.so", libraries / "libc++_shared.so")
    shutil.copy2(WORKER / "synthetic-input.json", assets / "synthetic-input.json")
    shutil.copy2(artifacts / "vkey.json", assets / "vkey.json")
    metadata = {"w2c2_revision": "9de3c2be5a4ed8ef5fdbd536e445120594fb8530", "files":
                {str(path.relative_to(output)): digest(path) for path in [
                    libraries / "libpersonhood_bench.so", libraries / "libc++_shared.so",
                    assets / "synthetic-input.json", assets / "vkey.json"]}}
    (output / "bundle.json").write_text(json.dumps(metadata, indent=2) + "\n")
    print(f"Bundle ready: {output}")
    print("Set PERSONHOOD_LAB_BUNDLE to that path, then run scripts/android-build.sh --debug --apk --features personhood-lab")


if __name__ == "__main__":
    main()
