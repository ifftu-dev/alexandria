# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project loosely follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
