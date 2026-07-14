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

/// Code Editor: JavaScript — graded coding element (v0.1.1: lazy wasm load, main
/// thread, instant-eval toggle). The learner writes JS in a
/// CodeMirror 6 editor; the same Boa wasm engine powers both in-browser live
/// eval (run on the iframe main thread from `ui/vendor/runner-wasm.js` — the
/// sandboxed opaque-origin iframe can't spawn a cross-origin `plugin://` Worker)
/// and the host-side credential grader (`editor-javascript-grader.wasm`, run in
/// the deterministic Wasmtime sandbox). The grader wasm is import-stubbed to zero
/// imports; `ui/vendor/*` are build-time produced by `editor-shared/build.sh`.
const EDITOR_JS_BUNDLE: BuiltinBundle<'static> = BuiltinBundle {
    slug: "editor-javascript",
    manifest_json: include_bytes!("../../../plugins/builtin/editor-javascript/manifest.json"),
    grader_wasm: Some(include_bytes!(
        "../../../plugins/builtin/editor-javascript/grader/dist/editor_javascript_grader.wasm"
    )),
    ui_files: &[
        (
            "ui/index.html",
            include_bytes!("../../../plugins/builtin/editor-javascript/ui/index.html"),
        ),
        (
            "ui/style.css",
            include_bytes!("../../../plugins/builtin/editor-javascript/ui/style.css"),
        ),
        (
            "ui/config.js",
            include_bytes!("../../../plugins/builtin/editor-javascript/ui/config.js"),
        ),
        (
            "ui/app.js",
            include_bytes!("../../../plugins/builtin/editor-javascript/ui/app.js"),
        ),
        (
            "ui/vendor/cm6.js",
            include_bytes!("../../../plugins/builtin/editor-javascript/ui/vendor/cm6.js"),
        ),
        (
            "ui/vendor/runner-wasm.js",
            include_bytes!("../../../plugins/builtin/editor-javascript/ui/vendor/runner-wasm.js"),
        ),
        (
            "icon.svg",
            include_bytes!("../../../plugins/builtin/editor-javascript/icon.svg"),
        ),
        (
            "README.md",
            include_bytes!("../../../plugins/builtin/editor-javascript/README.md"),
        ),
    ],
};

/// Code Editor: TypeScript — graded coding element. Identical to the JavaScript
/// editor except the Boa runner strips TypeScript types (bundled sucrase, run
/// in-engine) before executing. Same zero-import grader wasm + in-browser worker.
const EDITOR_TS_BUNDLE: BuiltinBundle<'static> = BuiltinBundle {
    slug: "editor-typescript",
    manifest_json: include_bytes!("../../../plugins/builtin/editor-typescript/manifest.json"),
    grader_wasm: Some(include_bytes!(
        "../../../plugins/builtin/editor-typescript/grader/dist/editor_typescript_grader.wasm"
    )),
    ui_files: &[
        (
            "ui/index.html",
            include_bytes!("../../../plugins/builtin/editor-typescript/ui/index.html"),
        ),
        (
            "ui/style.css",
            include_bytes!("../../../plugins/builtin/editor-typescript/ui/style.css"),
        ),
        (
            "ui/config.js",
            include_bytes!("../../../plugins/builtin/editor-typescript/ui/config.js"),
        ),
        (
            "ui/app.js",
            include_bytes!("../../../plugins/builtin/editor-typescript/ui/app.js"),
        ),
        (
            "ui/vendor/cm6.js",
            include_bytes!("../../../plugins/builtin/editor-typescript/ui/vendor/cm6.js"),
        ),
        (
            "ui/vendor/runner-wasm.js",
            include_bytes!("../../../plugins/builtin/editor-typescript/ui/vendor/runner-wasm.js"),
        ),
        (
            "icon.svg",
            include_bytes!("../../../plugins/builtin/editor-typescript/icon.svg"),
        ),
        (
            "README.md",
            include_bytes!("../../../plugins/builtin/editor-typescript/README.md"),
        ),
    ],
};

/// Code Editor: C++ — graded coding element. The learner writes C/C++,
/// interpreted by the bundled JSCPP engine running inside the same Boa wasm core
/// (both for in-browser live eval and the deterministic Wasmtime grader). Intro
/// subset only (no STL/templates).
const EDITOR_CPP_BUNDLE: BuiltinBundle<'static> = BuiltinBundle {
    slug: "editor-cpp",
    manifest_json: include_bytes!("../../../plugins/builtin/editor-cpp/manifest.json"),
    grader_wasm: Some(include_bytes!(
        "../../../plugins/builtin/editor-cpp/grader/dist/editor_cpp_grader.wasm"
    )),
    ui_files: &[
        (
            "ui/index.html",
            include_bytes!("../../../plugins/builtin/editor-cpp/ui/index.html"),
        ),
        (
            "ui/style.css",
            include_bytes!("../../../plugins/builtin/editor-cpp/ui/style.css"),
        ),
        (
            "ui/config.js",
            include_bytes!("../../../plugins/builtin/editor-cpp/ui/config.js"),
        ),
        (
            "ui/app.js",
            include_bytes!("../../../plugins/builtin/editor-cpp/ui/app.js"),
        ),
        (
            "ui/vendor/cm6.js",
            include_bytes!("../../../plugins/builtin/editor-cpp/ui/vendor/cm6.js"),
        ),
        (
            "ui/vendor/runner-wasm.js",
            include_bytes!("../../../plugins/builtin/editor-cpp/ui/vendor/runner-wasm.js"),
        ),
        (
            "icon.svg",
            include_bytes!("../../../plugins/builtin/editor-cpp/icon.svg"),
        ),
        (
            "README.md",
            include_bytes!("../../../plugins/builtin/editor-cpp/README.md"),
        ),
    ],
};

/// Code Editor: Python — graded coding element. The learner writes Python; it
/// runs on the RustPython VM (pure Rust) compiled to a zero-import wasm — both
/// for in-browser live eval and the deterministic Wasmtime grader. Teaching
/// subset (builtins only; no `import math`).
const EDITOR_PYTHON_BUNDLE: BuiltinBundle<'static> = BuiltinBundle {
    slug: "editor-python",
    manifest_json: include_bytes!("../../../plugins/builtin/editor-python/manifest.json"),
    grader_wasm: Some(include_bytes!(
        "../../../plugins/builtin/editor-python/grader/dist/editor_python_grader.wasm"
    )),
    ui_files: &[
        (
            "ui/index.html",
            include_bytes!("../../../plugins/builtin/editor-python/ui/index.html"),
        ),
        (
            "ui/style.css",
            include_bytes!("../../../plugins/builtin/editor-python/ui/style.css"),
        ),
        (
            "ui/config.js",
            include_bytes!("../../../plugins/builtin/editor-python/ui/config.js"),
        ),
        (
            "ui/app.js",
            include_bytes!("../../../plugins/builtin/editor-python/ui/app.js"),
        ),
        (
            "ui/vendor/cm6.js",
            include_bytes!("../../../plugins/builtin/editor-python/ui/vendor/cm6.js"),
        ),
        (
            "ui/vendor/runner-wasm.js",
            include_bytes!("../../../plugins/builtin/editor-python/ui/vendor/runner-wasm.js"),
        ),
        (
            "icon.svg",
            include_bytes!("../../../plugins/builtin/editor-python/icon.svg"),
        ),
        (
            "README.md",
            include_bytes!("../../../plugins/builtin/editor-python/README.md"),
        ),
    ],
};

/// Code Editors (collection) — runs no code itself; its manifest declares the
/// per-language editor plugins as `dependencies`, so installing it pulls them
/// in. Must be registered AFTER its dependencies in `BUILTIN_PLUGINS` so
/// dependency resolution finds them already installed. Interactive landing UI.
/// Course-scoped (like its editor deps): it is not installed at startup.
const EDITORS_BUNDLE: BuiltinBundle<'static> = BuiltinBundle {
    slug: "editors",
    manifest_json: include_bytes!("../../../plugins/builtin/editors/manifest.json"),
    grader_wasm: None,
    ui_files: &[
        (
            "ui/index.html",
            include_bytes!("../../../plugins/builtin/editors/ui/index.html"),
        ),
        (
            "ui/style.css",
            include_bytes!("../../../plugins/builtin/editors/ui/style.css"),
        ),
        (
            "ui/app.js",
            include_bytes!("../../../plugins/builtin/editors/ui/app.js"),
        ),
        (
            "icon.svg",
            include_bytes!("../../../plugins/builtin/editors/icon.svg"),
        ),
        (
            "README.md",
            include_bytes!("../../../plugins/builtin/editors/README.md"),
        ),
    ],
};

/// Every built-in plugin shipped with this binary. Mostly order-independent
/// (installs are idempotent), but a plugin that declares `dependencies` must
/// appear *after* the plugins it depends on, since dependency resolution at
/// install time requires the dependencies to already be installed. The
/// `editor-*` language plugins therefore precede the `editors` collection.
pub const BUILTIN_PLUGINS: &[BuiltinBundle<'static>] = &[
    MCQ_BUNDLE,
    MUSIC_TRAINER_BUNDLE,
    MUSIC_REVIEWS_BUNDLE,
    IRL_REVIEW_BUNDLE,
    EDITOR_JS_BUNDLE,
    EDITOR_TS_BUNDLE,
    EDITOR_CPP_BUNDLE,
    EDITOR_PYTHON_BUNDLE,
    // Collection last: it depends on the four editor plugins above.
    EDITORS_BUNDLE,
];

/// The builtin bundle whose manifest CID equals `cid`, if any. Used by the
/// enrollment flow to install a course's required builtin plugins on demand.
pub fn find_bundle_by_cid(cid: &str) -> Option<&'static BuiltinBundle<'static>> {
    BUILTIN_PLUGINS
        .iter()
        .find(|b| crate::plugins::verifier::compute_plugin_cid(b.manifest_json) == cid)
}

/// The builtin bundle whose manifest `id` (`did:key:<author>#<slug>`) equals
/// `id`, if any. Used to resolve a plugin's declared `dependencies` (which name
/// ids, not CIDs) to installable bundles.
pub fn find_bundle_by_id(id: &str) -> Option<&'static BuiltinBundle<'static>> {
    BUILTIN_PLUGINS.iter().find(|b| {
        manifest::parse_and_validate(b.manifest_json)
            .map(|m| m.id == id)
            .unwrap_or(false)
    })
}

/// Install every embedded *global*-scoped built-in plugin if not already
/// present. Called from `AppState::open_database` once the DB is unlocked and
/// migrated.
///
/// Course-scoped builtins (the code editors + the `editors` collection) are
/// **not** installed here — they install on first enrollment in a course that
/// requires them (see `commands::plugins::install_course_plugins`). The
/// exception is the `dev-seed` feature, under which every builtin is installed
/// so the seeded demo course works out of the box.
///
/// Errors on individual builtins are logged but do not fail the call —
/// a corrupt embedded bundle should not block app startup.
pub fn install_all(db: &Database, plugins_dir: &Path) -> InstallStats {
    use crate::domain::plugin::PluginScope;
    use crate::plugins::verifier;

    let mut stats = InstallStats::default();
    // Every builtin CID that legitimately belongs on this machine — used to
    // prune only *stale* builtin rows (old manifests). Includes course-scoped
    // CIDs even when not installed this run, so an editor a learner installed on
    // enrollment is never pruned on the next startup.
    let mut keep_cids: Vec<String> = Vec::with_capacity(BUILTIN_PLUGINS.len());
    for bundle in BUILTIN_PLUGINS {
        keep_cids.push(verifier::compute_plugin_cid(bundle.manifest_json));

        let scope = manifest::parse_and_validate(bundle.manifest_json)
            .map(|m| m.scope)
            .unwrap_or(PluginScope::Global);
        if scope == PluginScope::Course {
            log::debug!(
                "builtin plugin '{}' is course-scoped — deferred to enrollment",
                bundle.slug
            );
            continue;
        }

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

    // Prune stale builtin rows. When a builtin's manifest changes, its CID
    // changes, so the previous row lingers as a duplicate (old bundle, no
    // longer shipped). Drop any source='builtin' install whose CID isn't a
    // current builtin CID. Course-scoped installs survive (their CID is in
    // `keep_cids`); community plugins (other sources) are never touched.
    if !keep_cids.is_empty() {
        match registry::prune_builtins_except(db, plugins_dir, &keep_cids) {
            Ok(n) if n > 0 => log::info!("pruned {n} stale builtin plugin(s)"),
            Ok(_) => {}
            Err(e) => log::warn!("failed to prune stale builtins: {e}"),
        }
    }

    stats
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::verifier;
    use tempfile::TempDir;

    fn test_db() -> Database {
        let db = Database::open_in_memory().expect("db");
        db.run_migrations().expect("migrations");
        db
    }

    #[test]
    fn install_all_installs_global_builtins_without_failures() {
        let db = test_db();
        let dir = TempDir::new().unwrap();

        let stats = install_all(&db, dir.path());
        assert_eq!(stats.failed, 0, "no builtin should fail to install");
        // Without the `dev-seed` feature, startup installs only global-scoped
        // builtins; the course-scoped editor plugins + `editors` collection are
        // deferred to first enrollment. mcq/music×2/irl are the globals.
        let global_count = BUILTIN_PLUGINS
            .iter()
            .filter(|b| {
                manifest::parse_and_validate(b.manifest_json)
                    .map(|m| matches!(m.scope, crate::domain::plugin::PluginScope::Global))
                    .unwrap_or(true)
            })
            .count();
        assert_eq!(stats.installed, global_count);
    }

    #[test]
    fn editors_collection_records_editor_deps() {
        let db = test_db();
        let dir = TempDir::new().unwrap();

        // The `editors` collection declares the four language editors as
        // dependencies, so install them first (dependency resolution at install
        // time requires the dependencies to already be present), then the
        // collection, and assert the edges exist.
        for bundle in [
            EDITOR_JS_BUNDLE,
            EDITOR_TS_BUNDLE,
            EDITOR_CPP_BUNDLE,
            EDITOR_PYTHON_BUNDLE,
            EDITORS_BUNDLE,
        ] {
            registry::install_builtin(&db, dir.path(), &bundle).expect("install builtin");
        }

        let collection_cid = verifier::compute_plugin_cid(EDITORS_BUNDLE.manifest_json);
        let mut dep_names: Vec<String> = registry::list_dependencies(&db, &collection_cid)
            .unwrap()
            .into_iter()
            .map(|p| p.name)
            .collect();
        dep_names.sort();
        assert_eq!(
            dep_names,
            vec![
                "Code Editor: C++",
                "Code Editor: JavaScript",
                "Code Editor: Python",
                "Code Editor: TypeScript",
            ]
        );

        // Reverse edge: an editor reports the collection as a dependent, so a
        // user-facing uninstall of it would be refused while the collection is
        // installed.
        let js_cid = verifier::compute_plugin_cid(EDITOR_JS_BUNDLE.manifest_json);
        let dependents = registry::list_dependents(&db, &js_cid).unwrap();
        assert!(dependents.iter().any(|p| p.plugin_cid == collection_cid));
    }

    /// End-to-end wiring of the graded-submission path, minus the Tauri
    /// `AppState`: install the JavaScript editor builtin, read `grader.wasm`
    /// back off disk, re-verify its hash against the manifest (exactly as
    /// `plugin_submit_and_grade` does), then grade a correct and a wrong
    /// submission through the real Wasmtime sandbox and assert the scores.
    #[cfg(desktop)]
    #[test]
    fn editor_js_install_to_grade_wiring() {
        use crate::plugins::wasm_runtime::{GraderBudgets, GraderRuntime};

        let db = test_db();
        let dir = TempDir::new().unwrap();
        registry::install_builtin(&db, dir.path(), &EDITOR_JS_BUNDLE).expect("install js editor");

        let cid = verifier::compute_plugin_cid(EDITOR_JS_BUNDLE.manifest_json);
        let installed = registry::get_installed(&db, &cid)
            .unwrap()
            .expect("installed record");
        let manifest = registry::get_manifest(&db, &cid).unwrap();
        let grader_cid = manifest.grader.expect("editor has grader").cid;

        // Read the grader off disk and re-verify — the same tamper check the
        // command performs before every grade.
        let grader_path = Path::new(&installed.install_path).join(registry::GRADER_FILENAME);
        let wasm = std::fs::read(&grader_path).expect("grader.wasm on disk");
        assert_eq!(
            blake3::hash(&wasm).to_hex().to_string(),
            grader_cid,
            "on-disk grader hash must match the manifest"
        );

        // The demo "double the number" challenge: one visible + hidden tests.
        let content = serde_json::json!({
            "tests": [{ "name": "example", "stdin": "4", "expected_stdout": "8" }],
            "grader_private": { "tests": [
                { "name": "zero", "stdin": "0", "expected_stdout": "0" },
                { "name": "negative", "stdin": "-5", "expected_stdout": "-10" },
            ]}
        });
        let runtime = GraderRuntime::new().expect("runtime");
        let grade = |source: &str| {
            let input = serde_json::to_vec(&serde_json::json!({
                "version": "1",
                "content": content,
                "submission": { "source": source },
            }))
            .unwrap();
            runtime
                .grade(&grader_cid, &wasm, None, &input, GraderBudgets::default())
                .expect("grade")
                .score
        };

        // Correct solution passes every visible + hidden test.
        assert_eq!(
            grade("const n = Number(readLine()); console.log(n * 2);"),
            1.0
        );
        // A wrong solution scores below the credential threshold (0.7).
        assert!(grade("const n = Number(readLine()); console.log(n + 1);") < 0.7);
    }
}
