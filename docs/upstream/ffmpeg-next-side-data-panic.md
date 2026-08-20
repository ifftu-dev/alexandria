# Draft issue — ffmpeg-next

**Repo:** https://github.com/zmwangx/rust-ffmpeg
**Status:** drafted, not filed. Reviewed by: —
**Affects:** 9.0.0 (and 8.1.0, differently — see below)

---

**Title:** Unknown side-data types either fail to compile or panic; neither is safe for untrusted input

**Body:**

`Type::from(AVFrameSideDataType)` and its packet equivalent have no total
mapping for values the crate does not know about. Depending on the
`non-exhaustive-enums` feature, a value from a newer ffmpeg than the bindings
target does one of two things:

- **feature off** — the match is not exhaustive and the crate does not compile
  at all against the newer headers;
- **feature on** — `_ => unimplemented!()`, which panics at runtime.

Building 8.1 against ffmpeg 9 produces the first, thirteen errors deep in the
bindings, listing arms like `AV_PKT_DATA_HEVC_CONF` and
`AV_PKT_DATA_DYNAMIC_HDR_SMPTE_2094_APP5`. 9.0 with the feature on produces the
second.

### Why the panic is the more serious half

Side data is attached to frames and packets that arrive from *outside* the
process — in our case a remote peer's stream during a live video call. A
participant whose ffmpeg is newer, or who simply sends a stream carrying a side
data type this crate has not enumerated, takes the receiving process down. That
is a remote-triggerable panic reachable through ordinary use, and there is no
way for a caller to defend against it: the conversion happens inside the crate
before any of our code sees the frame.

`unimplemented!()` is a reasonable placeholder for an API the author intends to
finish. It is not a reasonable response to untrusted input, and the enum being
open-ended is precisely the case where untrusted input shows up.

### Suggested fix

Carry the unrecognised value instead of asserting it cannot happen:

```rust
pub enum Type {
    // …
    Unknown(AVFrameSideDataType),
}

// forward
value => Type::Unknown(value),

// reverse
Type::Unknown(value) => value,
```

This round-trips, needs no feature flag, compiles against any ffmpeg, and lets
callers ignore side data they do not care about — which is the overwhelmingly
common case — rather than crashing on it. It also removes the compile-time
coupling between the bindings' minor version and the ffmpeg headers present on
the build machine, which is the thing that makes upgrades painful today.

We carry this as a local patch. Happy to send it as a PR if the shape looks
right.
