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

/// Music Reviews — listens to a learner's instrument and marks each note
/// in a target sequence correct or wrong using live autocorrelation
/// pitch detection. Interactive (no grader). Requests microphone.
const MUSIC_REVIEWS_BUNDLE: BuiltinBundle<'static> = BuiltinBundle {
    slug: "music-reviews",
    manifest_json: include_bytes!("../../../plugins/builtin/music-reviews/manifest.json"),
    grader_wasm: None,
    ui_files: &[
        (
            "ui/index.html",
            include_bytes!("../../../plugins/builtin/music-reviews/ui/index.html"),
        ),
        (
            "ui/app.js",
            include_bytes!("../../../plugins/builtin/music-reviews/ui/app.js"),
        ),
        (
            "ui/pitch.js",
            include_bytes!("../../../plugins/builtin/music-reviews/ui/pitch.js"),
        ),
        (
            "icon.svg",
            include_bytes!("../../../plugins/builtin/music-reviews/icon.svg"),
        ),
        (
            "screenshots/timeline.svg",
            include_bytes!("../../../plugins/builtin/music-reviews/screenshots/timeline.svg"),
        ),
        (
            "README.md",
            include_bytes!("../../../plugins/builtin/music-reviews/README.md"),
        ),
    ],
};

/// IRL Review — learner submits work in any format to the local
/// instructor inbox; an instructor posts back a score, written feedback,
/// and per-skill ratings. Interactive (no grader). No capabilities.
const IRL_REVIEW_BUNDLE: BuiltinBundle<'static> = BuiltinBundle {
    slug: "irl-review",
    manifest_json: include_bytes!("../../../plugins/builtin/irl-review/manifest.json"),
    grader_wasm: None,
    ui_files: &[
        (
            "ui/index.html",
            include_bytes!("../../../plugins/builtin/irl-review/ui/index.html"),
        ),
        (
            "ui/app.js",
            include_bytes!("../../../plugins/builtin/irl-review/ui/app.js"),
        ),
        (
            "icon.svg",
            include_bytes!("../../../plugins/builtin/irl-review/icon.svg"),
        ),
        (
            "README.md",
            include_bytes!("../../../plugins/builtin/irl-review/README.md"),
        ),
    ],
};

/// Every built-in plugin shipped with this binary. Order is irrelevant —
/// installs are idempotent, so a future entry being added partway through
/// the list won't reorder anything.
pub const BUILTIN_PLUGINS: &[BuiltinBundle<'static>] = &[
    MCQ_BUNDLE,
    MUSIC_TRAINER_BUNDLE,
    MUSIC_REVIEWS_BUNDLE,
    IRL_REVIEW_BUNDLE,
];

/// Install every embedded built-in plugin if not already present. Called
/// from `AppState::open_database` once the DB is unlocked and migrated.
///
/// Errors on individual builtins are logged but do not fail the call —
/// a corrupt embedded bundle should not block app startup.
pub fn install_all(db: &Database, plugins_dir: &Path) -> InstallStats {
    let mut stats = InstallStats::default();
    let mut current_cids: Vec<String> = Vec::with_capacity(BUILTIN_PLUGINS.len());
    for bundle in BUILTIN_PLUGINS {
        match registry::install_builtin(db, plugins_dir, bundle) {
            Ok(plugin) => {
                stats.installed += 1;
                current_cids.push(plugin.plugin_cid.clone());
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

    // Prune stale builtin rows. When a builtin's manifest changes, its CID
    // changes, so the previous row lingers as a duplicate (old bundle, no
    // longer shipped). Drop any source='builtin' install whose CID isn't in
    // the set we just installed. Community plugins (other sources) are never
    // touched.
    if !current_cids.is_empty() {
        match registry::prune_builtins_except(db, plugins_dir, &current_cids) {
            Ok(n) if n > 0 => log::info!("pruned {n} stale builtin plugin(s)"),
            Ok(_) => {}
            Err(e) => log::warn!("failed to prune stale builtins: {e}"),
        }
    }

    stats
}
