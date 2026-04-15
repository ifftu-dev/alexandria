//! First-party plugin bundles embedded in the host binary.
//!
//! Phase 2 — every built-in element type the app ships is, architecturally,
//! a plugin. The bundle source lives at `plugins/builtin/<slug>/`; the
//! bytes are pulled in here via `include_bytes!` and installed at startup
//! through [`registry::install_builtin`]. Idempotent — same CID = no-op.
//!
//! Why this matters: it forces the plugin contract to be production-grade
//! on day one (the host's own UI flows through it) and gives us a single
//! dispatch path for built-ins and community plugins.

use std::path::Path;

use crate::db::Database;
use crate::plugins::catalog;
use crate::plugins::manifest;
use crate::plugins::registry::{self, BuiltinBundle, InstallStats};

/// MCQ — single + multi multiple-choice questions, graded by
/// `mcq-grader.wasm` in the deterministic Wasmtime sandbox.
const MCQ_BUNDLE: BuiltinBundle<'static> = BuiltinBundle {
    slug: "mcq",
    manifest_json: include_bytes!("../../../plugins/builtin/mcq/manifest.json"),
    grader_wasm: Some(include_bytes!(
        "../../../plugins/builtin/mcq-grader/dist/mcq_grader.wasm"
    )),
    ui_files: &[
        (
            "ui/index.html",
            include_bytes!("../../../plugins/builtin/mcq/ui/index.html"),
        ),
        (
            "ui/app.js",
            include_bytes!("../../../plugins/builtin/mcq/ui/app.js"),
        ),
    ],
};

/// Music Instrument Trainer — interactive demo of the capability-prompt
/// flow. Requests the microphone, displays a live amplitude meter. No
/// grader (interactive only). Acts as the canonical Phase 1 + Phase 3
/// end-to-end demo from the plan.
const MUSIC_TRAINER_BUNDLE: BuiltinBundle<'static> = BuiltinBundle {
    slug: "music-trainer",
    manifest_json: include_bytes!("../../../plugins/builtin/music-trainer/manifest.json"),
    grader_wasm: None,
    ui_files: &[
        (
            "ui/index.html",
            include_bytes!("../../../plugins/builtin/music-trainer/ui/index.html"),
        ),
        (
            "ui/app.js",
            include_bytes!("../../../plugins/builtin/music-trainer/ui/app.js"),
        ),
    ],
};

/// Every built-in plugin shipped with this binary. Order is irrelevant —
/// installs are idempotent, so a future entry being added partway through
/// the list won't reorder anything.
pub const BUILTIN_PLUGINS: &[BuiltinBundle<'static>] = &[MCQ_BUNDLE, MUSIC_TRAINER_BUNDLE];

/// Install every embedded built-in plugin if not already present. Called
/// from `AppState::open_database` once the DB is unlocked and migrated.
///
/// Errors on individual builtins are logged but do not fail the call —
/// a corrupt embedded bundle should not block app startup.
pub fn install_all(db: &Database, plugins_dir: &Path) -> InstallStats {
    let mut stats = InstallStats::default();
    for bundle in BUILTIN_PLUGINS {
        match registry::install_builtin(db, plugins_dir, bundle) {
            Ok(plugin) => {
                stats.installed += 1;
                log::info!(
                    "builtin plugin installed: {} v{} ({})",
                    plugin.name,
                    plugin.version,
                    plugin.plugin_cid
                );

                // Surface built-ins in the plugin catalog so the browse
                // UI lists them alongside community plugins.
                if let Ok(parsed) = manifest::parse_and_validate(bundle.manifest_json) {
                    let announcement = catalog::announcement_from_manifest(
                        &plugin.plugin_cid,
                        &parsed,
                        &plugin.installed_at,
                    );
                    if let Err(e) = catalog::upsert_announcement(db, &announcement, "builtin") {
                        log::warn!("failed to seed builtin '{}' into catalog: {e}", bundle.slug);
                    }
                }
            }
            Err(e) => {
                stats.failed += 1;
                log::warn!("builtin plugin '{}' failed to install: {e}", bundle.slug);
            }
        }
    }
    stats
}
