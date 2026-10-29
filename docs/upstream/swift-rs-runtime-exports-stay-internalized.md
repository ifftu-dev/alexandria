# Draft issue — swift-rs

Status: draft
Updated: 2026-10-29

Not yet filed upstream.

**Repo:** https://github.com/Brendonovich/swift-rs
**Affects:** 1.0.8 (Xcode 27), any consumer with a Swift package
**Our workaround:** `patches/swift-rs`, via `[patch.crates-io]`

---

**Title:** Xcode 27: the globalize step skips `SwiftRs.o`, so `retain_object` / `release_object` / `string_from_bytes` / `data_from_bytes` are undefined for every consumer

**Body:**

1.0.8 added `globalize_cdecl_symbols` to cope with Xcode 27's SwiftPM
internalizing `@_cdecl` exports in static products (`nm` shows them as local
`t`). It works for the package's own exports. It does not work for swift-rs's
own runtime, and the result is that no iOS target with a Swift package links:

```
Undefined symbols for architecture arm64:
  "_release_object", referenced from:
      swift_rs::swift::release_object in libswift_rs-….rlib
  "_retain_object", referenced from: …
  "_string_from_bytes", referenced from: …
  "_data_from_bytes", referenced from: …
```

### Cause

SwiftPM statically vendors this crate's Swift runtime into *every* package
archive as a `SwiftRs.o` member. The Rust side references four
of its `@_cdecl` exports. `globalize_cdecl_symbols` only promotes symbols from
the member whose name matches the package:

```rust
in_own_member = norm(module) == pkg;
…
if !in_own_member || kind != "t" || !name.starts_with('_') { continue; }
```

So in each archive — `libTauri.a`, `libtauri-plugin-dialog.a`, and so on —
`Tauri.o`'s exports are promoted and `SwiftRs.o`'s are not. Every archive
carries the four symbols, all of them local, and nothing in the whole link
defines them.

The own-member guard exists for a real reason (promoting dependency members in
every archive duplicates globals and crashes Xcode 27's `ld` with "malformed
atom files with duplicate names"), so the fix is not to drop it.

### Fix

Promote those four from `SwiftRs.o` in every archive **as weak** definitions.
Weak symbols coalesce instead of colliding, which is exactly the "one of these
identical copies wins" semantics the situation needs:

```rust
const SWIFT_RS_RUNTIME_MEMBER: &str = "swiftrs";
const SWIFT_RS_RUNTIME_EXPORTS: &[&str] =
    &["_retain_object", "_release_object", "_string_from_bytes", "_data_from_bytes"];
…
// in the nm loop, alongside in_own_member:
in_runtime_member = norm(module) == SWIFT_RS_RUNTIME_MEMBER;
…
if kind == "t" && in_runtime_member && SWIFT_RS_RUNTIME_EXPORTS.contains(&name) {
    weak_candidates.push(name);   // still subject to the uniqueness guard
    continue;
}
…
for s in &weak_syms {
    cmd.arg(format!("--globalize-symbol={s}"));
    cmd.arg(format!("--weaken-symbol={s}"));
}
```

After which `nm -m` shows:

```
weak private external _release_object
weak private external _retain_object
weak private external _string_from_bytes
weak private external _data_from_bytes
```

in each archive, and the link succeeds.

### Two related things worth a note in the README

- The whole step silently returns when rustup's `llvm-objcopy` is absent. The
  `cargo:warning` is easy to miss inside a Tauri build. Consumers on Xcode 27
  need `rustup component add llvm-tools`, and a hard error would save time.
- Fixing the archive after the fact does not help: rustc bundles the static
  library into the consuming rlib at compile time, so anything `objcopy` does
  to `libTauri.a` later is invisible to the link. The promotion has to happen
  in the build script, before rustc archives it — which is where
  `globalize_cdecl_symbols` already runs.

### Reproduction

- Tauri 2.x iOS app (any of the Swift-backed plugins is enough), swift-rs
  1.0.8, Xcode 27 beta, `rustup component add llvm-tools` done
- `cargo build --target aarch64-apple-ios --release`
- Link fails with the four undefined symbols above; `nm libTauri.a` shows
  them as `t` in `SwiftRs.o` while `Tauri.o`'s exports are `T`.

### Related release-link failure: duplicate shared modules

The 0.9.1-alpha iOS release failed under Xcode 26.3 with 323 duplicate
SwiftRs/Tauri symbols. Firebase's required `-ObjC` link flag exposed copies
of shared Swift modules embedded in each plugin archive. Existing cleanup
recognized the whole-module `Tauri.o` and `SwiftRs.o` members used by the
working local Xcode 27 build, but missed the native SwiftPM per-source layout,
including `Plugin.swift.o` and `lib.swift.o`.

The corrected `drop_shared_modules` also discovers object names from
`Tauri.build` and `SwiftRs.build` beside each archive. Only Tauri's archive
retains the shared modules. Cleanup rebuilds the archive index and rejects
ambiguous duplicate basenames before deleting anything, so a plugin's own
object cannot be accidentally removed. Archive-tool failures now stop the
build immediately.

On macOS, run `rustup component add llvm-tools`, then
[`scripts/test-swift-runtime.sh`](../../scripts/test-swift-runtime.sh).
The regression tests force-load shared and consumer archives, verify that both
layouts link without duplicate symbols, and execute the retained consumer
code. The per-source test failed before the fix and passes afterward; the
runtime export regression also passes. A real native SwiftPM fixture confirmed
the per-source layout and cleanup, and a full local Xcode 27 iOS release archive
succeeded. The iOS CI job now runs these linker tests on Xcode 26.3; a successful
Xcode 26.3 release archive remains the final validation boundary.
