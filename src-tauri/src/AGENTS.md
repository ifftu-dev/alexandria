# alexandria/src-tauri/src/ — Rust backend

Workspace rules and the general Rust conventions are in the workspace `AGENTS.md`. This file holds the backend gotchas.

- **One data set per profile**: each profile has its own vault, SQLCipher DB and iroh blob store under `<app_data>/profiles/<uuid>/`. `commands/profile.rs` owns the multi-user lifecycle; `commands/identity.rs` acts on the active profile only.
- **Profile open never deletes**: `ProfileManager::open_for_app` opens blocked (empty index, `create` refused) on an older index version or another network's profile; `move_incompatible_data_aside` moves that data to `incompatible-profile-data-<timestamp>/`. The pre-profile single-vault layout is detected and reported, not migrated.
- **Profile switch**: `lib.rs` `start_active_profile` / `stop_active_profile` bring services up and down without changing trust decisions. `ContentNode::set_data_dir` repoints the blob store on each unlock, and `ContentNode::shutdown` must call both `Router::shutdown` and `Store::shutdown` so the redb lock releases between switches.
- **Network profile**: `network_profile.rs` validates `../resources/networks/preprod.json` and the embedded-resource digest; setup fails on either mismatch.
- **Built-in data**: `db/bundled.rs` upserts the taxonomy, goal templates and question banks on every profile open, overwriting local edits to bundled rows. Release builds also install bundled course drafts, plugins and classroom templates (`commands/demo_courses.rs`, `commands/demo_resources.rs`); dev builds offer the opt-in picker in `commands/dev_seeds.rs`. No seed creates personas, credentials or enrollments.
- **Settings**: `settings/registry.rs` keys drive the per-profile `app_settings` table; `scope='sync'` rows travel through `p2p::sync` (`settings_outbound_snapshot` / `settings_apply_inbound`).
- **Retired authority**: challenge/escrow command and builder paths are gone, and leftover challenge/plugin-attestation tables grant nothing. Credential status changes are issuer-bound.
- **Tests**: FK insert order is `subject_fields → subjects → skills → …`; per-module helpers (`test_db()`, `setup_identity()`, `insert_course()`) already exist, so reuse them.
- **Cardano**: reference-script UTxOs deployed on preprod are recorded in `cardano/script_refs.rs`.
