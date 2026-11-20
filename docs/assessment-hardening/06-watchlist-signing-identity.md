# 06 Watchlist matches a tool's signing identity

Triage: ready-for-agent
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

- [ ] Pure-function tests: a renamed process with a watched identity matches; an unsigned process falls back to name; an unwatched identity with a watched-looking name still matches by name.
- [ ] macOS and Windows identity readers compile under their `cfg`; Linux build unaffected.
- [ ] fmt, clippy pass.

## Pointers

- `src-tauri/src/sentinel/processes.rs` (`MatchKind::NameContains`, the watchlist), its existing tests.
