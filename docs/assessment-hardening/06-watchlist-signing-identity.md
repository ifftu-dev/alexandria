# 06 Watchlist matches a tool's signing identity

Triage: done
Type: task
Repos: alexandria
Part of: alexandria/docs/assessment-hardening-spec.md
Blocked by: none

Spec sections: *Watchlist*, story 45.

## What to build

- A watchlist entry may name a signing identity: the team identifier on macOS, the publisher on Windows. A running process whose signature carries a watched identity matches whatever its file name.
- Unsigned processes, and Linux, keep name matching.
- A match raises the same flag at the same severity as a name match.
- Reading a signature must stay cheap on old machines: read it once per process identifier and cache it, and never block the sampling loop.
- Fill in signing identities for watchlist entries whose vendors publish one, where it can be confirmed from public sources; leave the rest name-only.

## Acceptance criteria

- [x] Pure-function tests: a renamed process with a watched identity matches; an unsigned process falls back to name; an unwatched identity with a watched-looking name still matches by name.
- [x] macOS and Windows identity readers compile under their `cfg`; Linux build unaffected.
- [x] fmt, clippy pass.

## Pointers

- `src-tauri/src/sentinel/processes.rs` (`MatchKind::NameContains`, the watchlist), its existing tests.

## Notes

2026-11-20: built.

- `MatchKind::Signer` compares a process's signing identity (lowercased) with the needle; rules report as `signer:<needle>`. `RawProcess` gains an optional `signer`, omitted from JSON when absent. A match is an ordinary watchlist hit in the rule's category, so flags and severity are unchanged. Nothing new crosses IPC: `WatchedProcess` is unchanged.
- Signer rules sit after their vendor's name and id rules, so an unrenamed tool keeps reporting the rule it reported before.
- macOS reads the team identifier with `SecStaticCodeCreateWithPath` + `SecCodeCopySigningInformation` (raw FFI, no new crate); Windows reads the publisher of the embedded Authenticode signature with `CryptQueryObject` + `CertGetNameStringW` (new `windows` feature `Win32_Security_Cryptography`). Neither validates the signature: a stripped or re-signed binary, and catalog-signed Windows files, fall back to name matching. Linux and mobile set no signer.
- One background thread reads each executable path once into a process-wide cache. A scan never waits: a path first seen in a scan matches by name until its read lands, normally by the next sample. On a dev Mac with 700 processes the first scan took 20 ms with no signers and later scans about 4 ms with them.
- Identities added, each confirmed from a public source (listed in the watchlist's provenance comment): macOS team ids for TeamViewer, AnyDesk, RustDesk, Parsec, OBS, ManyCam, Zoom, Discord, OpenAI, Anthropic and Ollama; Windows publishers for TeamViewer and AnyDesk (current and former names). Microsoft and Google stay name-only because their identities also sign tools that are deliberately not watched. The Anthropic and OpenAI ids also match their command-line tools as `ai_assistant`.
- Deviation: the Windows reader was checked with clippy for `x86_64-pc-windows-msvc` in a standalone crate pinned to the same `windows` version, because the app's C dependencies do not cross-compile from macOS. It has not run on a Windows machine; that check joins the existing owner task for device attestation.
