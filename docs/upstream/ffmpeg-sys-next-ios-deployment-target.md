# Draft issue — ffmpeg-sys-next

**Repo:** https://github.com/zmwangx/rust-ffmpeg-sys
**Status:** drafted, not filed. Reviewed by: —
**Affects:** 8.1.0 and 9.0.0 (checked both)

---

**Title:** `IPHONEOS_DEPLOYMENT_TARGET` leaks into the ffmpeg build, killing native macOS builds with `Killed: 9`

**Body:**

Building `ffmpeg-sys-next` with the `build` feature on a macOS host fails during
`./configure` with no usable error:

```
configure: line 1685: 61420 Killed: 9   $TMPE >> $logfile 2>&1
C compiler test failed.
```

The build script panics on the unwrap that follows, so what a user sees is a
dead configure and a backtrace pointing at `build.rs`.

### Cause

`configure` compiles and *runs* a small probe to check the C compiler works. If
`IPHONEOS_DEPLOYMENT_TARGET` is set in the environment, clang targets iPhone —
while still using the macOS sysroot — so the probe binary is built for iOS, the
kernel refuses to execute it on the host, and configure reports only that the
compiler test failed.

The variable is easy to have set without realising it applies here. Cargo's
`[env]` table is a common way to pin the deployment target for the `cc` crate,
which only applies it when the target is actually iOS. clang has no such
restriction: it honours the variable whenever it is invoked directly, which is
exactly what `./configure` does.

Reproduction on an Apple Silicon host:

```sh
IPHONEOS_DEPLOYMENT_TARGET=16.4 cargo build --features build
```

Symptom is identical whether or not the crate's own iOS features are enabled,
and it is not an out-of-memory condition, which is the usual first guess given
the `Killed: 9`.

### Fix

Strip the variable from the child environment unless the target really is iOS:

```rust
if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("ios") {
    configure.env_remove("IPHONEOS_DEPLOYMENT_TARGET");
}
```

`make` needs the same treatment. Removing it from `configure` alone gets past
the probe and then compiles every object file for iOS anyway, because `make` is
a separate process that inherits the environment too — ffmpeg builds cleanly all
the way to a link that fails with `building for 'macOS', but linking in object
file built for 'iOS'`. A complete, successful build of the wrong architecture is
a worse failure than the first one, since nothing is obviously wrong until the
very end.

### Related

While here: `apple_version_min_cflag` is applied only when `target != host`, so
a *native* macOS build passes no `-mmacosx-version-min` at all. ffmpeg then
compiles against the host SDK's default while the final binary links at
rustc's minimum, and clang lowers the `@available` guards around VideoToolbox
into runtime checks calling `___isPlatformVersionAtLeast`. That symbol lives in
compiler-rt, which clang links automatically and rustc does not, so the
*application* fails to link with an undefined symbol pointing into
`videotoolbox.o` — again after ffmpeg itself has built without complaint.
