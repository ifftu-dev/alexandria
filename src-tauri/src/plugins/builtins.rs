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

/// codejudge: Lua — interactive coding-challenge element. The learner writes
/// Lua; it runs locally in the bundled wasmoon VM (Lua 5.4 compiled to wasm,
/// no JS eval, no network) against the problem's test cases. No grader.
///
/// The `ui/vendor/*` and `ui/problems.js` files are build-time fetched/baked by
/// `plugins/builtin/codejudge-shared/fetch-runtimes.sh lua` — run it before
/// building or these `include_bytes!` paths won't exist.
const CODEJUDGE_LUA_BUNDLE: BuiltinBundle<'static> = BuiltinBundle {
    slug: "codejudge-lua",
    manifest_json: include_bytes!("../../../plugins/builtin/codejudge-lua/manifest.json"),
    grader_wasm: None,
    ui_files: &[
        (
            "ui/index.html",
            include_bytes!("../../../plugins/builtin/codejudge-lua/ui/index.html"),
        ),
        (
            "ui/style.css",
            include_bytes!("../../../plugins/builtin/codejudge-lua/ui/style.css"),
        ),
        (
            "ui/app.js",
            include_bytes!("../../../plugins/builtin/codejudge-lua/ui/app.js"),
        ),
        (
            "ui/runner.js",
            include_bytes!("../../../plugins/builtin/codejudge-lua/ui/runner.js"),
        ),
        (
            "ui/problems.js",
            include_bytes!("../../../plugins/builtin/codejudge-lua/ui/problems.js"),
        ),
        (
            "ui/vendor/codemirror.js",
            include_bytes!("../../../plugins/builtin/codejudge-lua/ui/vendor/codemirror.js"),
        ),
        (
            "ui/vendor/codemirror.css",
            include_bytes!("../../../plugins/builtin/codejudge-lua/ui/vendor/codemirror.css"),
        ),
        (
            "ui/vendor/theme-material-darker.css",
            include_bytes!(
                "../../../plugins/builtin/codejudge-lua/ui/vendor/theme-material-darker.css"
            ),
        ),
        (
            "ui/vendor/mode.js",
            include_bytes!("../../../plugins/builtin/codejudge-lua/ui/vendor/mode.js"),
        ),
        (
            "ui/vendor/lua.js",
            include_bytes!("../../../plugins/builtin/codejudge-lua/ui/vendor/lua.js"),
        ),
        (
            "icon.svg",
            include_bytes!("../../../plugins/builtin/codejudge-lua/icon.svg"),
        ),
        (
            "README.md",
            include_bytes!("../../../plugins/builtin/codejudge-lua/README.md"),
        ),
    ],
};

/// codejudge: JavaScript — interactive coding-challenge element. The learner
/// writes JS; it runs locally in the bundled QuickJS WebAssembly engine (wasm
/// embedded as base64, no eval, no network) against the problem's test cases.
/// No grader (interactive only). `ui/vendor/*` and `ui/problems.js` are
/// build-time produced by `fetch-runtimes.sh javascript`.
const CODEJUDGE_JS_BUNDLE: BuiltinBundle<'static> = BuiltinBundle {
    slug: "codejudge-javascript",
    manifest_json: include_bytes!("../../../plugins/builtin/codejudge-javascript/manifest.json"),
    grader_wasm: None,
    ui_files: &[
        (
            "ui/index.html",
            include_bytes!("../../../plugins/builtin/codejudge-javascript/ui/index.html"),
        ),
        (
            "ui/style.css",
            include_bytes!("../../../plugins/builtin/codejudge-javascript/ui/style.css"),
        ),
        (
            "ui/app.js",
            include_bytes!("../../../plugins/builtin/codejudge-javascript/ui/app.js"),
        ),
        (
            "ui/runner.js",
            include_bytes!("../../../plugins/builtin/codejudge-javascript/ui/runner.js"),
        ),
        (
            "ui/problems.js",
            include_bytes!("../../../plugins/builtin/codejudge-javascript/ui/problems.js"),
        ),
        (
            "ui/vendor/codemirror.js",
            include_bytes!("../../../plugins/builtin/codejudge-javascript/ui/vendor/codemirror.js"),
        ),
        (
            "ui/vendor/codemirror.css",
            include_bytes!(
                "../../../plugins/builtin/codejudge-javascript/ui/vendor/codemirror.css"
            ),
        ),
        (
            "ui/vendor/theme-material-darker.css",
            include_bytes!(
                "../../../plugins/builtin/codejudge-javascript/ui/vendor/theme-material-darker.css"
            ),
        ),
        (
            "ui/vendor/mode.js",
            include_bytes!("../../../plugins/builtin/codejudge-javascript/ui/vendor/mode.js"),
        ),
        (
            "ui/vendor/quickjs.js",
            include_bytes!("../../../plugins/builtin/codejudge-javascript/ui/vendor/quickjs.js"),
        ),
        (
            "icon.svg",
            include_bytes!("../../../plugins/builtin/codejudge-javascript/icon.svg"),
        ),
        (
            "README.md",
            include_bytes!("../../../plugins/builtin/codejudge-javascript/README.md"),
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

/// codejudge (umbrella) — runs no code itself; its manifest declares the
/// per-language judge plugins as `dependencies`, so installing it pulls them
/// in. Must be registered AFTER its dependencies in `BUILTIN_PLUGINS` so
/// dependency resolution finds them already installed. Interactive landing UI.
const CODEJUDGE_MULTILANG_BUNDLE: BuiltinBundle<'static> = BuiltinBundle {
    slug: "codejudge-multilang",
    manifest_json: include_bytes!("../../../plugins/builtin/codejudge-multilang/manifest.json"),
    grader_wasm: None,
    ui_files: &[
        (
            "ui/index.html",
            include_bytes!("../../../plugins/builtin/codejudge-multilang/ui/index.html"),
        ),
        (
            "ui/style.css",
            include_bytes!("../../../plugins/builtin/codejudge-multilang/ui/style.css"),
        ),
        (
            "ui/app.js",
            include_bytes!("../../../plugins/builtin/codejudge-multilang/ui/app.js"),
        ),
        (
            "ui/problems.js",
            include_bytes!("../../../plugins/builtin/codejudge-multilang/ui/problems.js"),
        ),
        (
            "icon.svg",
            include_bytes!("../../../plugins/builtin/codejudge-multilang/icon.svg"),
        ),
        (
            "README.md",
            include_bytes!("../../../plugins/builtin/codejudge-multilang/README.md"),
        ),
    ],
};

/// Every built-in plugin shipped with this binary. Mostly order-independent
/// (installs are idempotent), but a plugin that declares `dependencies` must
/// appear *after* the plugins it depends on, since dependency resolution at
/// install time requires the dependencies to already be installed. The
/// codejudge language plugins therefore precede the `codejudge-multilang`
/// umbrella.
pub const BUILTIN_PLUGINS: &[BuiltinBundle<'static>] = &[
    MCQ_BUNDLE,
    MUSIC_TRAINER_BUNDLE,
    MUSIC_REVIEWS_BUNDLE,
    IRL_REVIEW_BUNDLE,
    CODEJUDGE_LUA_BUNDLE,
    CODEJUDGE_JS_BUNDLE,
    EDITOR_JS_BUNDLE,
    EDITOR_TS_BUNDLE,
    EDITOR_CPP_BUNDLE,
    EDITOR_PYTHON_BUNDLE,
    // Umbrella last: it depends on the two language plugins above.
    CODEJUDGE_MULTILANG_BUNDLE,
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
    fn install_all_installs_builtins_and_records_codejudge_deps() {
        let db = test_db();
        let dir = TempDir::new().unwrap();

        let stats = install_all(&db, dir.path());
        assert_eq!(stats.failed, 0, "no builtin should fail to install");
        assert_eq!(stats.installed, BUILTIN_PLUGINS.len());

        // The umbrella declares the two language plugins as dependencies; after
        // install_all (umbrella registered last) those edges must exist.
        let umbrella_cid = verifier::compute_plugin_cid(CODEJUDGE_MULTILANG_BUNDLE.manifest_json);
        let mut dep_names: Vec<String> = registry::list_dependencies(&db, &umbrella_cid)
            .unwrap()
            .into_iter()
            .map(|p| p.name)
            .collect();
        dep_names.sort();
        assert_eq!(dep_names, vec!["codejudge: JavaScript", "codejudge: Lua"]);

        // Reverse edge: the Lua plugin reports the umbrella as a dependent, so
        // a user-facing uninstall of it would be refused while the umbrella is
        // installed.
        let lua_cid = verifier::compute_plugin_cid(CODEJUDGE_LUA_BUNDLE.manifest_json);
        let dependents = registry::list_dependents(&db, &lua_cid).unwrap();
        assert!(dependents.iter().any(|p| p.plugin_cid == umbrella_cid));
    }
}
