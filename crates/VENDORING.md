# Vendored third-party crates

`crates/iroh-moq`, `crates/moq-media` and `crates/live` are **not original
Alexandria work**. They derive from [`n0-computer/iroh-live`][upstream], which is
Copyright (C) 2025 N0, INC and licensed `MIT OR Apache-2.0`.

Alexandria distributes them under the **Apache License 2.0**, a copy of which sits
in each of those three directories as `LICENSE-APACHE`. (Upstream offers a dual
grant, but its `LICENSE-MIT` file is byte-identical to `LICENSE-APACHE` — only the
Apache text is actually supplied — so electing Apache-2.0 is the unambiguous
reading. Apache-2.0 is compatible with the AGPL-3.0-or-later that covers the rest
of this repository.)

This file exists because Apache-2.0 §4 requires it in substance: recipients get
the licence, modified files say they were modified, and upstream's copyright and
attribution notices are retained rather than quietly absorbed.

## Provenance chain

| Step | Where | Reference |
|---|---|---|
| Upstream | `github.com/n0-computer/iroh-live` | Copyright (C) 2025 N0, INC |
| Alexandria's working copy | `github.com/ifftu-dev/iroh-live` | standalone repo, created 2026-03-05 |
| Revision vendored from | `ifftu-dev/iroh-live` | `a7e3e0fa8c073021d7e140813197705eb0f670fd` (2026-03-26) |
| Vendored into this repo | commit `19bb430` | *"feat(storage): iroh 1.0 P2P storage layer + in-tree MoQ crates"* |

`ifftu-dev/iroh-live` is a clone rather than a GitHub fork, so the platform
records no parent link. The upstream relationship is real regardless, and this
table is the record of it.

Upstream ships no `NOTICE` file, so Apache-2.0 §4(d) has nothing to propagate.

## What is upstream's and what is ours

**The per-file `// Copyright 2025 N0, INC` header is the authority.** A file
carrying it is N0's work, whether or not Alexandria has since edited it. Those
headers must not be removed.

- **`crates/iroh-moq`** (~490 lines) — entirely N0's, unmodified since import.
- **`crates/moq-media`** (~9,000 lines) — overwhelmingly N0's: the MoQ
  container and catalog plumbing, the ffmpeg codec layer, Opus, the audio
  backends, and the VideoToolbox codecs. Alexandria wrote `src/android/mod.rs`
  and `src/android/camera.rs` (the NDK Camera2 backend, ~720 lines, written
  because no Rust capture crate supports Android) and has modified a number of
  N0's files in place.
- **`crates/live`** (~770 lines) — Alexandria's, and it *replaced* the vendored
  `iroh-live` crate rather than wrapping it (commit `19d3293`). It is listed here
  anyway because `src/rooms.rs` is a port of N0's room layer rather than a
  clean-room implementation, which makes it a derivative work.

Do not compare this tree against upstream's current `main` to infer authorship.
Upstream restructured after the March 2026 fork point — `pipeline/`,
`adaptive.rs`, `transport.rs` and others do not correspond to anything here — so
a path-level diff reports N0's own files as though they were new.

## Modifications

Alexandria has modified this code in two phases, and only the second is visible
in this repository's history:

1. **In `ifftu-dev/iroh-live`, before `19bb430`.** Not enumerable from here. The
   clearest surviving trace is `src/videotoolbox/camera.rs`, which arrived at
   import already containing `AlexandriaFrameDelegate` and an
   `org.alexandria.camera` dispatch queue.
2. **In this repository, since `19bb430`.** `git diff 19bb430 HEAD --
   crates/moq-media` is authoritative. As of this writing: `Cargo.toml`,
   `src/lib.rs`, `src/audio.rs`, `src/capture.rs`, `src/ffmpeg/video/encoder.rs`,
   `src/ffmpeg/video/util.rs`, `src/videotoolbox/decoder.rs`, and the
   Alexandria-authored `src/android/`.

Because phase 1 cannot be enumerated precisely, **treat every file in
`crates/moq-media` as potentially modified from upstream.** That is the honest
position and the one this notice takes.

## If you are adding to these crates

Keep N0's copyright headers. Add a modification notice beneath the existing
header rather than replacing it. New files that contain no upstream code get an
Alexandria copyright header instead, as `src/android/` does.

Taking an upstream fix requires reconciling by hand — there is no shared history
with `n0-computer/iroh-live` to rebase onto.

[upstream]: https://github.com/n0-computer/iroh-live
