# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project loosely follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [2026-03-25]

### Added
- **Classrooms**: Full classroom/cohort management with 24 Tauri commands.
  - Create/archive classrooms, manage members with role-based access (owner/moderator/member).
  - Text and announcement channels with real-time P2P messaging via per-classroom gossip topics.
  - Join request workflow (request, approve, deny).
  - Voice/video calls via iroh-live integration (desktop), with mobile stubs.
  - 4 frontend pages: classroom list, detail/messages, settings, join requests.
  - `useClassroom` composable with singleton state and real-time Tauri event listeners.
- **Live Tutoring**: Video/audio/screenshare sessions via iroh-live.
  - TutoringManager with platform-specific variants (desktop, mobile, Android).
  - 14 commands: create/join/leave rooms, toggle video/audio/screenshare, chat.
  - TutoringPiP component for picture-in-picture call overlay.
  - Database tables: tutoring_sessions, tutoring_peers, tutoring_chat.
- **Security remediation**: 21 of 27 audit findings fixed.
  - DOMPurify sanitization on all `v-html` sites (XSS prevention).
  - Restrictive Content Security Policy enabled in Tauri.
  - Wallet struct implements Drop with zeroization; Clone removed.
  - 12-character minimum password enforcement.
  - Salt file HMAC-SHA256 integrity protection (desktop + mobile keystores).
  - Per-peer token-bucket rate limiter for gossip messages.
  - Committee updates wrapped in transactions.
  - Proposal status validated against allowlist.
  - Sync column names validated against injection.
  - Raw `p2p_publish` command removed from IPC surface.
  - SSRF blocklist for IPFS content resolver.
  - Biometric session password auto-clears after 15 minutes.
  - Mutex `.unwrap()` replaced with `.map_err()` across all command handlers.
- **CI**: Security audit job with `cargo-audit` and updater key placeholder check.
- **Fonts**: Inter and JetBrains Mono bundled locally (removed Google Fonts CDN dependency).

### Fixed
- Mobile tab bar "Classrooms" linked to `/courses` instead of `/classrooms`.
- Onboarding password hint said "8 characters" but backend enforced 12.

## [2026-03-03]

### Added
- Public content availability flow for fresh installs and cross-device access:
  - URL-based content IDs and resolver support with local BLAKE3 caching.
  - `bootstrap_public_catalog` and `hydrate_catalog_courses` commands.
  - Bundled public catalog dataset (`bootstrap/public_courses.json`).
- Home post-unlock content sync attempt with bottom-bar status messaging and completion stats.

### Changed
- Mobile tab bar remains fixed to the required four tabs: Home, Live Tutoring, Classrooms, Skill Graph.

### Notes
- PR #43 was an intermediate stacked PR that closed when its base branch merged.
- Equivalent changes were merged through PR #44.
