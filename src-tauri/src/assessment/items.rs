//! Unified assessment items.
//!
//! One *item* is one gradeable thing — a multiple-choice question, a coding
//! exercise, an essay, a submitted git repository — and every kind is graded
//! by a deterministic WASM grader through the frozen ABI v1 in
//! [`crate::plugins::wasm_runtime`].
//!
//! Two properties follow, and they are the reason this module exists rather
//! than each kind carrying its own grading path:
//!
//! * **Scores are re-derivable.** Grading records
//!   `(grader_cid, content_cid, submission_cid)`. Anyone holding those three
//!   can re-run the grader and get a byte-identical score, without trusting
//!   the device that produced it. That is what makes an Alexandria
//!   credential checkable by a third party.
//! * **A new item kind is a new plugin.** Adding essay or repository grading
//!   is a wasm bundle, not a change to the host.
//!
//! # The content split
//!
//! An item stores its content in two halves:
//!
//! * `content_public` — prompt, options, starter code. May be sent to a
//!   client.
//! * `grader_private` — answer keys, hidden test cases. Must never be sent
//!   to a client.
//!
//! The grade envelope is the **flat merge** `content_public ∪ grader_private`,
//! with the private half winning on key collisions. Flat rather than nested
//! so each grader keeps its own convention for where private material sits:
//! `mcq-grader` reads a top-level `correct_indices`, while the Boa-based
//! editor graders read a nested `grader_private.tests`. Storing
//! `{"correct_indices": [...]}` for the former and
//! `{"grader_private": {...}}` for the latter makes one host rule serve both,
//! and no grader has to be rebuilt to fit this module.
//!
//! # Option shuffling
//!
//! MCQ options are shuffled per attempt, so a client submits *served
//! positions* while the key is in *original indices*. The mapping back
//! happens here, host-side, before the envelope is built. The grader stays a
//! pure function of original indices and never learns that shuffling exists.
//!
//! Consequently `content_cid` is computed over the unshuffled content and is
//! stable for an item across every attempt — which is what a verifier needs
//! in order to re-derive a score from the item alone.

use serde::{Deserialize, Serialize};

use crate::db::Database;
use crate::plugins::grade_contract::ScoreRecord;
use crate::plugins::registry;

#[cfg(desktop)]
use crate::plugins::grade_contract::GradeInput;
#[cfg(desktop)]
use crate::plugins::wasm_runtime::{GraderBudgets, GraderRuntime};

/// Grading engine for one call.
///
/// Wasmtime has no mobile target, so the runtime is `#[cfg(desktop)]`. This
/// carries it where it exists and is empty where it does not, keeping one
/// `grade_item` signature on every platform instead of two cfg'd copies of
/// the whole function. Both variants take a lifetime so that signature is
/// literally identical — a mobile-only unit struct would not compile against
/// `&GradeEngine<'_>`.
#[cfg(desktop)]
pub struct GradeEngine<'a> {
    pub runtime: &'a GraderRuntime,
    pub budgets: GraderBudgets,
}

/// Mobile: no wasm engine. MCQ still grades natively; plugin items refuse,
/// matching the existing `GraderUnavailable` behaviour.
#[cfg(not(desktop))]
#[derive(Default)]
pub struct GradeEngine<'a>(std::marker::PhantomData<&'a ()>);

/// Kind of gradeable item. `Mcq` is host-provided and graded by the built-in
/// `mcq-grader`; `Plugin` delegates to the grader named by the item's
/// `plugin_cid`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    Mcq,
    Plugin,
}

impl ItemKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ItemKind::Mcq => "mcq",
            ItemKind::Plugin => "plugin",
        }
    }

    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "mcq" => Ok(ItemKind::Mcq),
            "plugin" => Ok(ItemKind::Plugin),
            other => Err(format!("unknown assessment item kind '{other}'")),
        }
    }
}

/// One assessment item as stored. `grader_private` is loaded only on the
/// host's grading path — see [`served_item`] for the client-facing view.
#[derive(Debug, Clone)]
pub struct AssessmentItem {
    pub id: String,
    pub item_kind: ItemKind,
    pub skill_id: String,
    pub plugin_cid: Option<String>,
    pub content_public: serde_json::Value,
    pub grader_private: Option<serde_json::Value>,
    pub difficulty: u8,
    pub points: f64,
}

/// The half of an item that may cross the IPC boundary. Deliberately a
/// distinct type: it is not possible to hand a client an [`AssessmentItem`]
/// by accident, because the client-facing commands only ever speak this.
#[derive(Debug, Clone, Serialize)]
pub struct ServedItem {
    pub id: String,
    pub item_kind: ItemKind,
    pub content: serde_json::Value,
}

/// Result of grading one item, including everything a third party needs in
/// order to re-derive the score.
#[derive(Debug, Clone, Serialize)]
pub struct ItemGrade {
    pub score: f64,
    pub details: serde_json::Value,
    pub grader_cid: String,
    pub content_cid: String,
    pub submission_cid: String,
}

/// Load an item by id. Returns `Ok(None)` when it does not exist.
pub fn load_item(db: &Database, item_id: &str) -> Result<Option<AssessmentItem>, String> {
    let row = db
        .conn()
        .query_row(
            "SELECT id, item_kind, skill_id, plugin_cid, content_public, grader_private, \
                    difficulty, points \
               FROM assessment_items WHERE id = ?1",
            [item_id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, Option<String>>(5)?,
                    r.get::<_, i64>(6)?,
                    r.get::<_, f64>(7)?,
                ))
            },
        )
        .map(Some)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(other.to_string()),
        })?;

    let Some((id, kind, skill_id, plugin_cid, public, private, difficulty, points)) = row else {
        return Ok(None);
    };

    Ok(Some(AssessmentItem {
        id,
        item_kind: ItemKind::parse(&kind)?,
        skill_id,
        plugin_cid,
        content_public: serde_json::from_str(&public)
            .map_err(|e| format!("item content_public is not valid JSON: {e}"))?,
        grader_private: private
            .map(|p| serde_json::from_str(&p))
            .transpose()
            .map_err(|e| format!("item grader_private is not valid JSON: {e}"))?,
        difficulty: difficulty.clamp(1, 5) as u8,
        points,
    }))
}

/// Client-facing projection. Carries `content_public` only — the key stays
/// on the host.
///
/// `option_order` (when present) re-orders MCQ options into the shuffled
/// order this attempt serves. The client sees options in shuffled order and
/// submits positions into that order; [`grade_item`] maps them back.
pub fn served_item(item: &AssessmentItem, option_order: Option<&[usize]>) -> ServedItem {
    let mut content = item.content_public.clone();

    if let (Some(order), Some(obj)) = (option_order, content.as_object_mut()) {
        if let Some(serde_json::Value::Array(options)) = obj.get("options") {
            let shuffled: Vec<serde_json::Value> = order
                .iter()
                .filter_map(|&orig| options.get(orig).cloned())
                .collect();
            obj.insert("options".into(), serde_json::Value::Array(shuffled));
        }
    }

    ServedItem {
        id: item.id.clone(),
        item_kind: item.item_kind,
        content,
    }
}

/// Build the grade envelope's `content`: `content_public ∪ grader_private`,
/// private winning on collision. See the module docs for why this is a flat
/// merge.
pub fn grade_content(item: &AssessmentItem) -> serde_json::Value {
    let mut content = item.content_public.clone();

    let (Some(obj), Some(private)) = (content.as_object_mut(), item.grader_private.as_ref()) else {
        return content;
    };
    let Some(private_obj) = private.as_object() else {
        return content;
    };

    for (k, v) in private_obj {
        obj.insert(k.clone(), v.clone());
    }
    content
}

/// Map MCQ served positions back to original option indices.
///
/// `option_order[served_position] = original_index`. Positions outside the
/// order are dropped rather than erroring: a client that submits garbage
/// scores badly, it does not break grading. The result is sorted and
/// deduplicated so the envelope is canonical — two clients submitting the
/// same set in different orders must hash identically.
pub fn map_selection_to_original(selected_positions: &[usize], option_order: &[usize]) -> Vec<u32> {
    let mut mapped: Vec<u32> = selected_positions
        .iter()
        .filter_map(|&pos| option_order.get(pos).map(|&orig| orig as u32))
        .collect();
    mapped.sort_unstable();
    mapped.dedup();
    mapped
}

/// Where a grader's bytes live on disk, plus the CID that must match them.
struct ResolvedGrader {
    cid: String,
    install_path: String,
}

/// Resolve which grader runs an item.
///
/// `Mcq` items resolve to the built-in `mcq` plugin, which is installed at
/// startup — its CID is derived from the embedded manifest bytes, so it
/// cannot be recorded in a migration that runs before installation.
fn resolve_grader(db: &Database, item: &AssessmentItem) -> Result<ResolvedGrader, String> {
    let plugin_cid = match item.item_kind {
        ItemKind::Mcq => crate::plugins::builtins::mcq_plugin_cid(),
        ItemKind::Plugin => item
            .plugin_cid
            .clone()
            .ok_or_else(|| format!("plugin item {} has no plugin_cid", item.id))?,
    };

    let installed = registry::get_installed(db, &plugin_cid)?
        .ok_or_else(|| format!("grader plugin not installed: {plugin_cid}"))?;
    let manifest = registry::get_manifest(db, &plugin_cid)?;
    let grader = manifest
        .grader
        .as_ref()
        .ok_or_else(|| format!("plugin {plugin_cid} declares no grader"))?;

    Ok(ResolvedGrader {
        cid: grader.cid.clone(),
        install_path: installed.install_path,
    })
}

/// Grade one item.
///
/// `submission` is the raw client submission. For MCQ it must carry
/// `selected_positions` (served positions); this function maps those to
/// original indices using `option_order` and rewrites them as
/// `selected_indices`, which is what the grader reads. Other kinds pass
/// their submission through untouched.
///
/// The returned CIDs are BLAKE3 over the exact bytes handed to the grader,
/// so a verifier re-running with the same three inputs gets the same score.
pub fn grade_item(
    db: &Database,
    engine: &GradeEngine<'_>,
    item: &AssessmentItem,
    submission: &serde_json::Value,
    option_order: Option<&[usize]>,
) -> Result<ItemGrade, String> {
    let resolved = resolve_grader(db, item)?;

    let submission = normalize_submission(item, submission, option_order)?;
    let content = grade_content(item);

    // Serialize each half once and hash exactly those bytes, so the CIDs
    // describe what the grader actually saw.
    let content_bytes = serde_json::to_vec(&content).map_err(|e| e.to_string())?;
    let submission_bytes = serde_json::to_vec(&submission).map_err(|e| e.to_string())?;
    let content_cid = blake3::hash(&content_bytes).to_hex().to_string();
    let submission_cid = blake3::hash(&submission_bytes).to_hex().to_string();

    let record = run_grader(engine, &resolved, item, &content, &submission)?;

    Ok(ItemGrade {
        // A grader returning out-of-range is a bug in that grader, not a
        // reason to reject the attempt — clamp and carry on.
        score: record.score.clamp(0.0, 1.0),
        details: record.details,
        grader_cid: resolved.cid,
        content_cid,
        submission_cid,
    })
}

/// Desktop: run the published wasm artifact.
#[cfg(desktop)]
fn run_grader(
    engine: &GradeEngine<'_>,
    resolved: &ResolvedGrader,
    item: &AssessmentItem,
    content: &serde_json::Value,
    submission: &serde_json::Value,
) -> Result<ScoreRecord, String> {
    use std::path::Path;

    let wasm_path = Path::new(&resolved.install_path).join(registry::GRADER_FILENAME);
    let cwasm_path = Path::new(&resolved.install_path).join(registry::grader_cwasm_filename());

    let wasm_bytes = std::fs::read(&wasm_path)
        .map_err(|e| format!("failed to read grader.wasm for item {}: {e}", item.id))?;

    // Re-check the bytes against the declared CID. Install already verified
    // the manifest signature, but a grader.wasm swapped on disk next to a
    // valid manifest would otherwise grade unnoticed.
    let computed = blake3::hash(&wasm_bytes).to_hex().to_string();
    if computed != resolved.cid {
        return Err(format!(
            "grader.wasm hash mismatch for item {}: manifest declared {}, on-disk is {computed}",
            item.id, resolved.cid
        ));
    }

    let envelope = GradeInput {
        version: "1".to_string(),
        content: content.clone(),
        submission: submission.clone(),
    };
    let envelope_bytes = serde_json::to_vec(&envelope).map_err(|e| e.to_string())?;

    engine.runtime.grade(
        &resolved.cid,
        &wasm_bytes,
        Some(&cwasm_path),
        &envelope_bytes,
        engine.budgets,
    )
}

/// Mobile: no wasm engine exists. MCQ grades through the native
/// implementation, which [`crate::assessment::mcq`] proves equivalent to the
/// wasm artifact whose CID is recorded — so a score produced here still
/// re-derives correctly elsewhere. Plugin items cannot run and say so.
#[cfg(not(desktop))]
fn run_grader(
    _engine: &GradeEngine<'_>,
    _resolved: &ResolvedGrader,
    item: &AssessmentItem,
    content: &serde_json::Value,
    submission: &serde_json::Value,
) -> Result<ScoreRecord, String> {
    match item.item_kind {
        ItemKind::Mcq => Ok(crate::assessment::mcq::score(content, submission)),
        ItemKind::Plugin => Err(
            "GraderUnavailable: this item is graded by a plugin, which runs on the desktop app; \
             open it there to submit for a credential"
                .to_string(),
        ),
    }
}

/// Rewrite a submission into the shape its grader expects.
fn normalize_submission(
    item: &AssessmentItem,
    submission: &serde_json::Value,
    option_order: Option<&[usize]>,
) -> Result<serde_json::Value, String> {
    if item.item_kind != ItemKind::Mcq {
        return Ok(submission.clone());
    }

    let positions: Vec<usize> = submission
        .get("selected_positions")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_u64().map(|n| n as usize))
                .collect()
        })
        .unwrap_or_default();

    // Identity order when the attempt did not shuffle (served positions are
    // already original indices).
    let identity: Vec<usize>;
    let order = match option_order {
        Some(o) => o,
        None => {
            let len = item
                .content_public
                .get("options")
                .and_then(|v| v.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            identity = (0..len).collect();
            &identity
        }
    };

    Ok(serde_json::json!({
        "selected_indices": map_selection_to_original(&positions, order),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mcq(correct: &[u32], options: usize) -> AssessmentItem {
        let opts: Vec<String> = (0..options).map(|i| format!("opt{i}")).collect();
        AssessmentItem {
            id: "item_1".into(),
            item_kind: ItemKind::Mcq,
            skill_id: "skill_x".into(),
            plugin_cid: None,
            content_public: serde_json::json!({
                "kind": if correct.len() == 1 { "single" } else { "multi" },
                "prompt": "q?",
                "options": opts,
            }),
            grader_private: Some(serde_json::json!({ "correct_indices": correct })),
            difficulty: 2,
            points: 1.0,
        }
    }

    #[test]
    fn served_item_never_carries_the_key() {
        // The whole point of the public/private split. If this fails, the
        // answer key is one serialization away from the client.
        let item = mcq(&[2], 4);
        let served = served_item(&item, None);
        let json = serde_json::to_string(&served).unwrap();
        assert!(
            !json.contains("correct_indices"),
            "served item leaks the key"
        );
        assert!(
            !json.contains("grader_private"),
            "served item leaks private content"
        );
    }

    #[test]
    fn grade_content_merges_private_over_public() {
        let item = mcq(&[1, 3], 4);
        let content = grade_content(&item);
        assert_eq!(
            content.get("correct_indices").unwrap(),
            &serde_json::json!([1, 3])
        );
        // Public keys survive the merge.
        assert_eq!(content.get("kind").unwrap(), "multi");
        assert!(content.get("options").is_some());
    }

    #[test]
    fn grade_content_supports_nested_private_conventions() {
        // Boa-based editor graders read `grader_private.tests`. A flat merge
        // of a `{"grader_private": {...}}` blob reproduces that nesting, so
        // one host rule serves both conventions.
        let mut item = mcq(&[0], 2);
        item.grader_private = Some(serde_json::json!({
            "grader_private": { "tests": [{ "expected_stdout": "4" }] }
        }));
        let content = grade_content(&item);
        assert!(content
            .get("grader_private")
            .and_then(|g| g.get("tests"))
            .is_some());
    }

    #[test]
    fn served_options_follow_the_shuffle() {
        let item = mcq(&[0], 4);
        let served = served_item(&item, Some(&[2, 0, 3, 1]));
        let opts = served.content.get("options").unwrap().as_array().unwrap();
        assert_eq!(opts[0], "opt2");
        assert_eq!(opts[1], "opt0");
        assert_eq!(opts[3], "opt1");
    }

    #[test]
    fn selection_maps_served_positions_to_original_indices() {
        // order[served] = original. Selecting served 0 means original 2.
        assert_eq!(map_selection_to_original(&[0], &[2, 0, 3, 1]), vec![2]);
        assert_eq!(
            map_selection_to_original(&[1, 3], &[2, 0, 3, 1]),
            vec![0, 1]
        );
    }

    #[test]
    fn selection_is_canonical_regardless_of_submit_order() {
        // Two clients picking the same options in different orders must
        // produce identical bytes, or submission_cid stops being a
        // function of the answer.
        let a = map_selection_to_original(&[3, 1], &[2, 0, 3, 1]);
        let b = map_selection_to_original(&[1, 3], &[2, 0, 3, 1]);
        assert_eq!(a, b);
    }

    #[test]
    fn selection_drops_out_of_range_positions() {
        assert_eq!(map_selection_to_original(&[0, 99], &[2, 0, 3, 1]), vec![2]);
        assert_eq!(map_selection_to_original(&[7], &[2, 0]), Vec::<u32>::new());
    }

    #[test]
    fn normalize_rewrites_positions_into_original_indices() {
        let item = mcq(&[2], 4);
        let submitted = serde_json::json!({ "selected_positions": [0] });
        let out = normalize_submission(&item, &submitted, Some(&[2, 0, 3, 1])).unwrap();
        assert_eq!(out, serde_json::json!({ "selected_indices": [2] }));
    }

    #[test]
    fn normalize_without_shuffle_treats_positions_as_original() {
        let item = mcq(&[2], 4);
        let submitted = serde_json::json!({ "selected_positions": [2] });
        let out = normalize_submission(&item, &submitted, None).unwrap();
        assert_eq!(out, serde_json::json!({ "selected_indices": [2] }));
    }

    #[test]
    fn normalize_passes_non_mcq_submissions_through() {
        let mut item = mcq(&[0], 2);
        item.item_kind = ItemKind::Plugin;
        item.plugin_cid = Some("cid".into());
        let submitted = serde_json::json!({ "source": "print(1)" });
        let out = normalize_submission(&item, &submitted, None).unwrap();
        assert_eq!(out, submitted);
    }

    #[test]
    fn missing_selection_grades_as_empty_not_error() {
        // An unanswered question is a wrong answer, not a failed attempt.
        let item = mcq(&[1], 3);
        let out = normalize_submission(&item, &serde_json::json!({}), None).unwrap();
        assert_eq!(out, serde_json::json!({ "selected_indices": [] }));
    }

    #[test]
    fn item_kind_round_trips() {
        for k in [ItemKind::Mcq, ItemKind::Plugin] {
            assert_eq!(ItemKind::parse(k.as_str()).unwrap(), k);
        }
        assert!(ItemKind::parse("nope").is_err());
    }
}

/// End-to-end: a row in `assessment_items` graded through the real built-in
/// MCQ plugin, installed the way the app installs it at startup.
///
/// The unit tests above cover envelope construction in isolation; these
/// prove the pieces are actually wired to each other — that
/// [`resolve_grader`] finds the built-in plugin by its derived CID, that the
/// bytes on disk verify, and that a shuffled attempt grades correctly.
#[cfg(all(test, desktop))]
mod e2e {
    use super::*;
    use crate::plugins::wasm_runtime::{GraderBudgets, GraderRuntime};
    use crate::plugins::{builtins, registry};
    use tempfile::TempDir;

    struct Fixture {
        db: Database,
        runtime: GraderRuntime,
        _dir: TempDir,
    }

    impl Fixture {
        fn engine(&self) -> GradeEngine<'_> {
            GradeEngine {
                runtime: &self.runtime,
                budgets: GraderBudgets::default(),
            }
        }
    }

    fn fixture() -> Fixture {
        let db = Database::open_in_memory().expect("db");
        db.run_migrations().expect("migrations");
        let dir = TempDir::new().expect("tempdir");

        // Install exactly the bundle the app ships, through the same path.
        let mcq = builtins::BUILTIN_PLUGINS
            .iter()
            .find(|b| b.slug == "mcq")
            .expect("mcq bundle is a builtin");
        registry::install_builtin(&db, dir.path(), mcq).expect("install mcq");

        Fixture {
            db,
            runtime: GraderRuntime::new().expect("runtime"),
            _dir: dir,
        }
    }

    /// Insert a bank + question and run the backfill, so the item under test
    /// arrives the same way a real migrated item would.
    fn insert_migrated_item(db: &Database, id: &str, correct: &str, options: &str) {
        db.conn()
            .execute(
                "INSERT OR IGNORE INTO question_banks (id, skill_id, label, ratified) \
                 VALUES ('bank_e2e', 'skill_rust', 'E2E', 1)",
                [],
            )
            .expect("bank");
        db.conn()
            .execute(
                "INSERT INTO bank_questions (id, bank_id, prompt, options, correct_indices) \
                 VALUES (?1, 'bank_e2e', 'Which?', ?2, ?3)",
                rusqlite::params![id, options, correct],
            )
            .expect("question");
        // Re-run migration 072 (idempotent) so the item arrives through the
        // real backfill rather than a hand-written INSERT that could drift.
        let (_, _, sql) = crate::db::schema::MIGRATIONS
            .iter()
            .find(|(v, _, _)| *v == 72)
            .expect("migration 072 exists");
        db.conn().execute_batch(sql).expect("backfill");
    }

    fn grade(
        f: &Fixture,
        item_id: &str,
        positions: &[usize],
        order: Option<&[usize]>,
    ) -> ItemGrade {
        let item = load_item(&f.db, item_id)
            .expect("load")
            .expect("item exists");
        grade_item(
            &f.db,
            &f.engine(),
            &item,
            &serde_json::json!({ "selected_positions": positions }),
            order,
        )
        .expect("grade succeeds")
    }

    #[test]
    fn migrated_item_grades_through_the_builtin_grader() {
        let f = fixture();
        insert_migrated_item(&f.db, "q_e2e", "[0]", r#"["fn","func","def","lambda"]"#);

        let right = grade(&f, "q_e2e", &[0], None);
        assert_eq!(right.score, 1.0);
        let wrong = grade(&f, "q_e2e", &[2], None);
        assert_eq!(wrong.score, 0.0);
    }

    #[test]
    fn grading_records_the_reproducibility_triple() {
        // Without all three CIDs a score cannot be re-derived by anyone who
        // does not trust this device — which is the property the whole
        // unification exists to buy.
        let f = fixture();
        insert_migrated_item(&f.db, "q_triple", "[1]", r#"["a","b","c"]"#);

        let g = grade(&f, "q_triple", &[1], None);
        assert_eq!(g.grader_cid.len(), 64, "grader cid is a blake3 hex digest");
        assert_eq!(g.content_cid.len(), 64);
        assert_eq!(g.submission_cid.len(), 64);
        assert_eq!(
            g.grader_cid,
            registry::get_manifest(&f.db, &builtins::mcq_plugin_cid())
                .unwrap()
                .grader
                .unwrap()
                .cid,
            "recorded grader must be the one the manifest declares"
        );
    }

    #[test]
    fn content_cid_is_stable_across_attempts_and_shuffles() {
        // A verifier re-derives from the item alone, so per-attempt
        // shuffling must not change the content it hashes.
        let f = fixture();
        insert_migrated_item(&f.db, "q_stable", "[0]", r#"["a","b","c","d"]"#);

        let plain = grade(&f, "q_stable", &[0], None);
        let shuffled = grade(&f, "q_stable", &[2], Some(&[1, 2, 0, 3]));
        assert_eq!(plain.content_cid, shuffled.content_cid);
    }

    #[test]
    fn shuffled_attempt_grades_against_original_indices() {
        let f = fixture();
        insert_migrated_item(&f.db, "q_shuf", "[2]", r#"["a","b","c","d"]"#);

        // order[served] = original. Original 2 sits at served position 1.
        let order = [0usize, 2, 1, 3];
        assert_eq!(grade(&f, "q_shuf", &[1], Some(&order)).score, 1.0);
        assert_eq!(grade(&f, "q_shuf", &[0], Some(&order)).score, 0.0);
    }

    #[test]
    fn identical_answers_hash_identically() {
        // submission_cid must be a function of the answer, not of the order
        // the client happened to click in.
        let f = fixture();
        insert_migrated_item(&f.db, "q_hash", "[0,2]", r#"["a","b","c","d"]"#);

        let a = grade(&f, "q_hash", &[0, 2], None);
        let b = grade(&f, "q_hash", &[2, 0], None);
        assert_eq!(a.submission_cid, b.submission_cid);
        assert_eq!(a.score, b.score);
    }

    #[test]
    fn grading_is_byte_reproducible() {
        let f = fixture();
        insert_migrated_item(&f.db, "q_repro", "[1,3]", r#"["a","b","c","d"]"#);

        let first = grade(&f, "q_repro", &[1], None);
        for _ in 0..5 {
            let again = grade(&f, "q_repro", &[1], None);
            assert_eq!(first.score, again.score);
            assert_eq!(first.content_cid, again.content_cid);
            assert_eq!(first.submission_cid, again.submission_cid);
            assert_eq!(first.grader_cid, again.grader_cid);
        }
    }

    #[test]
    fn multi_select_earns_partial_credit_end_to_end() {
        let f = fixture();
        insert_migrated_item(&f.db, "q_partial", "[0,1]", r#"["a","b","c","d"]"#);

        assert_eq!(grade(&f, "q_partial", &[0, 1], None).score, 1.0);
        assert_eq!(grade(&f, "q_partial", &[0], None).score, 0.5);
        assert_eq!(grade(&f, "q_partial", &[0, 2], None).score, 0.0);
    }

    #[test]
    fn plugin_item_without_a_plugin_cid_is_rejected() {
        let f = fixture();
        f.db.conn()
            .execute(
                "INSERT INTO assessment_items (id, item_kind, skill_id, content_public) \
                 VALUES ('q_bad', 'plugin', 'skill_rust', '{}')",
                [],
            )
            .expect("insert");

        let item = load_item(&f.db, "q_bad").unwrap().unwrap();
        let err = grade_item(&f.db, &f.engine(), &item, &serde_json::json!({}), None).unwrap_err();
        assert!(err.contains("no plugin_cid"), "unexpected error: {err}");
    }

    #[test]
    fn missing_item_is_none_not_an_error() {
        let f = fixture();
        assert!(load_item(&f.db, "nope").expect("query ok").is_none());
    }
}

/// Migration equivalence: the unified wasm path versus the host grader it
/// replaces.
///
/// Migration 072 moves MCQ scoring from [`crate::assessment::grader`]
/// (exact-set match, 1.0 or 0.0) onto the built-in `mcq-grader` wasm, which
/// awards partial credit on multi-select. That is a deliberate behaviour
/// change and these tests pin exactly how far it goes, because it decides
/// whether historical attempts would have been graded differently.
///
/// Two properties, checked exhaustively over every key/selection pair for a
/// four-option question:
///
/// 1. `wasm >= host` — unification can only raise a score. A pass verdict
///    can therefore flip fail→pass but never pass→fail, so no already-issued
///    credential is retroactively invalidated.
/// 2. `wasm == 1.0` exactly when `host == 1.0` — full marks are awarded on
///    precisely the same answers as before. Partial credit fills the gap
///    between 0 and 1; it never redefines "correct".
#[cfg(all(test, desktop))]
mod equivalence {
    use crate::assessment::grader::{grade as host_grade, GradedQuestion};
    use crate::plugins::wasm_runtime::{GraderBudgets, GraderRuntime};

    const MCQ_GRADER_WASM: &[u8] =
        include_bytes!("../../../plugins/builtin/mcq-grader/dist/mcq_grader.wasm");

    const OPTIONS: usize = 4;

    /// Score one MCQ through the real wasm grader.
    fn wasm_score(runtime: &GraderRuntime, correct: &[u32], selected: &[u32]) -> f64 {
        let kind = if correct.len() == 1 {
            "single"
        } else {
            "multi"
        };
        let opts: Vec<String> = (0..OPTIONS).map(|i| format!("opt{i}")).collect();
        let input = serde_json::to_vec(&serde_json::json!({
            "version": "1",
            "content": { "kind": kind, "options": opts, "correct_indices": correct },
            "submission": { "selected_indices": selected },
        }))
        .unwrap();

        let cid = blake3::hash(MCQ_GRADER_WASM).to_hex().to_string();
        runtime
            .grade(
                &cid,
                MCQ_GRADER_WASM,
                None,
                &input,
                GraderBudgets::default(),
            )
            .expect("mcq grader runs")
            .score
    }

    /// Score the same MCQ through the host grader being replaced. Identity
    /// option order, so served positions are original indices.
    fn host_score(correct: &[u32], selected: &[u32]) -> f64 {
        let q = GradedQuestion {
            points: 1.0,
            correct_indices: correct.iter().map(|&i| i as usize).collect(),
            option_order: (0..OPTIONS).collect(),
        };
        let answer: Vec<usize> = selected.iter().map(|&i| i as usize).collect();
        host_grade(&[q], &[answer])
    }

    /// Every non-empty subset of `0..OPTIONS`, as index vectors.
    fn subsets() -> Vec<Vec<u32>> {
        (1u32..(1 << OPTIONS))
            .map(|mask| {
                (0..OPTIONS as u32)
                    .filter(|i| mask & (1 << i) != 0)
                    .collect()
            })
            .collect()
    }

    #[test]
    fn unification_never_lowers_a_score() {
        let runtime = GraderRuntime::new().expect("runtime");
        for correct in subsets() {
            for selected in subsets() {
                let w = wasm_score(&runtime, &correct, &selected);
                let h = host_score(&correct, &selected);
                assert!(
                    w >= h - 1e-9,
                    "wasm scored lower than host: correct={correct:?} selected={selected:?} \
                     wasm={w} host={h}"
                );
            }
        }
    }

    #[test]
    fn full_marks_agree_exactly() {
        let runtime = GraderRuntime::new().expect("runtime");
        for correct in subsets() {
            for selected in subsets() {
                let w = wasm_score(&runtime, &correct, &selected);
                let h = host_score(&correct, &selected);
                assert_eq!(
                    w >= 1.0 - 1e-9,
                    h >= 1.0 - 1e-9,
                    "full-marks disagreement: correct={correct:?} selected={selected:?} \
                     wasm={w} host={h}"
                );
            }
        }
    }

    #[test]
    fn empty_selection_scores_zero_in_both() {
        let runtime = GraderRuntime::new().expect("runtime");
        for correct in subsets() {
            assert_eq!(wasm_score(&runtime, &correct, &[]), 0.0);
            assert_eq!(host_score(&correct, &[]), 0.0);
        }
    }

    #[test]
    fn single_answer_items_are_bit_identical() {
        // Where |correct| == 1 the two graders should agree on every input,
        // not merely bound each other — most existing bank questions are
        // single-answer, so this is the bulk of the migration.
        let runtime = GraderRuntime::new().expect("runtime");
        for c in 0..OPTIONS as u32 {
            for selected in subsets() {
                let w = wasm_score(&runtime, &[c], &selected);
                let h = host_score(&[c], &selected);
                assert_eq!(
                    w, h,
                    "single-answer mismatch: correct=[{c}] selected={selected:?}"
                );
            }
        }
    }

    #[test]
    fn partial_credit_is_the_only_difference() {
        // Concretely: on a 2-of-4 multi where the learner gets one right and
        // adds nothing wrong, the old grader gave 0 and the new one gives
        // half. This is the behaviour change, stated as a fact rather than
        // left implicit in a bound.
        let runtime = GraderRuntime::new().expect("runtime");
        assert_eq!(host_score(&[1, 3], &[1]), 0.0);
        assert_eq!(wasm_score(&runtime, &[1, 3], &[1]), 0.5);

        // And a wrong extra selection cancels a right one.
        assert_eq!(wasm_score(&runtime, &[1, 3], &[1, 0]), 0.0);
    }
}
