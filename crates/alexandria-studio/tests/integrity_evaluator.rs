//! Seam 4: the offline evaluator over a corpus manifest.
//!
//! Expected values are worked by hand from the committed synthetic fixture
//! (`fixtures/integrity-corpus/`), never recomputed the way the evaluator
//! does: core has 5 attack sessions (14 windows) and 4 clean ones
//! (11 windows); camera has 2 of each.

use std::path::{Path, PathBuf};

use alexandria_studio::integrity::measurement::cli::{self, Outcome};
use alexandria_studio::integrity::measurement::{
    evaluate, Certification, Corpus, MeasurementError,
};
use alexandria_studio::integrity::RatioManifest;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/integrity-corpus/manifest.json")
}

fn close(actual: Option<f64>, expected: f64) -> bool {
    actual.is_some_and(|a| (a - expected).abs() < 1e-4)
}

#[test]
fn per_signal_rates_come_from_the_labelled_sessions() {
    let corpus = Corpus::load(&fixture()).unwrap();
    let report = evaluate(&corpus, &RatioManifest::uncertified());

    let core = &report.profiles["core"];
    assert_eq!((core.attack_sessions, core.clean_sessions), (5, 4));

    let paste = &core.signals["paste_detected"];
    assert_eq!((paste.fired_attack, paste.fired_clean), (2, 0));
    assert!(close(paste.precision, 1.0));
    assert!(close(paste.recall, 0.4));
    // ln((3.5 / 15) / (0.5 / 12)) = ln 5.6
    assert!(close(paste.llr, 1.7228));

    let switch = &core.signals["app_switch"];
    assert!(close(switch.precision, 2.0 / 3.0));
    assert!(close(switch.recall, 0.4));
    assert!(close(switch.false_positive_rate, 0.25));
    // ln((3.5 / 15) / (1.5 / 12)) = ln 1.8667
    assert!(close(switch.llr, 0.6242));

    let glance = &report.profiles["camera"].signals["device_glance"];
    assert!(close(glance.precision, 0.5));
    assert!(close(glance.recall, 0.5));
}

/// A writable copy of the fixture corpus.
fn copy_fixture() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("integrity-corpus-{}", uuid::Uuid::new_v4()));
    let src = fixture().parent().unwrap().to_path_buf();
    std::fs::create_dir_all(dir.join("entries")).unwrap();
    std::fs::copy(src.join("manifest.json"), dir.join("manifest.json")).unwrap();
    for file in std::fs::read_dir(src.join("entries")).unwrap() {
        let file = file.unwrap();
        std::fs::copy(file.path(), dir.join("entries").join(file.file_name())).unwrap();
    }
    dir
}

#[test]
fn an_entry_edited_after_sealing_is_refused() {
    let dir = copy_fixture();
    let entry = dir.join("entries/clean-core-macos-01.json");
    let edited = std::fs::read_to_string(&entry)
        .unwrap()
        .replace("\"clean\"", "\"paste_answer\"");
    std::fs::write(&entry, edited).unwrap();

    let error = Corpus::load(&dir.join("manifest.json")).unwrap_err();
    assert!(
        matches!(error, MeasurementError::DigestMismatch { .. }),
        "{error}"
    );
}

fn certification() -> PathBuf {
    fixture()
        .parent()
        .unwrap()
        .join("certification.synthetic.json")
}

fn run(args: &[&str]) -> (Result<Outcome, MeasurementError>, String) {
    let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    let mut out = Vec::new();
    let outcome = cli::run(&args, &mut out);
    (outcome, String::from_utf8(out).unwrap())
}

fn path(p: &Path) -> &str {
    p.to_str().unwrap()
}

/// The printed row for `signal` under `profile`, split on whitespace.
fn row(out: &str, profile: &str, signal: &str) -> Vec<String> {
    let section = out
        .split("\nprofile ")
        .find(|s| s.starts_with(&format!("{profile}:")))
        .unwrap_or_else(|| panic!("no profile {profile} in\n{out}"));
    section
        .lines()
        .find(|l| l.split_whitespace().next() == Some(signal))
        .unwrap_or_else(|| panic!("no row {signal} in\n{section}"))
        .split_whitespace()
        .map(str::to_string)
        .collect()
}

#[test]
fn the_ci_check_passes_and_prints_the_certified_rates() {
    let (outcome, out) = run(&["check", path(&fixture()), path(&certification())]);
    assert_eq!(outcome.unwrap(), Outcome::Pass, "{out}");
    assert!(out.contains("SYNTHETIC"), "{out}");
    // signal, precision, recall, fpr, llr, status
    assert_eq!(
        row(&out, "core", "paste_detected"),
        [
            "paste_detected",
            "1.000",
            "0.400",
            "0.000",
            "1.723",
            "admitted"
        ]
    );
    assert_eq!(
        row(&out, "core", "app_switch"),
        ["app_switch", "0.667", "0.400", "0.250", "0.624", "admitted"]
    );
    assert_eq!(
        row(&out, "core", "in_app_assistant_during_attempt")[5],
        "advisory"
    );
}

#[test]
fn the_confusion_matrix_shows_fusion_outcomes_per_label() {
    let (outcome, out) = run(&[
        "report",
        path(&fixture()),
        "--certification",
        path(&certification()),
    ]);
    assert_eq!(outcome.unwrap(), Outcome::Pass);
    // label, completed, review, flagged, suspended. One warning flag is
    // review, never more; only the backend tamper flag suspends.
    assert_eq!(row(&out, "core", "clean")[1..], ["3", "1", "0", "0"]);
    assert_eq!(row(&out, "core", "paste_answer")[1..], ["0", "2", "0", "0"]);
    assert_eq!(
        row(&out, "core", "in_app_assistant")[1..],
        ["0", "0", "0", "1"]
    );
    assert_eq!(row(&out, "camera", "clean")[1..], ["1", "1", "0", "0"]);
}

#[test]
fn a_draft_certification_admits_the_measured_signals_and_holds_on_its_corpus() {
    let (outcome, out) = run(&[
        "draft",
        path(&fixture()),
        "--version",
        "draft-1",
        "--date",
        "2026-11-12",
        "--threshold",
        "core=2.5",
    ]);
    assert_eq!(outcome.unwrap(), Outcome::Pass);
    let draft: Certification = serde_json::from_str(&out).unwrap();
    assert!(
        draft.synthetic,
        "a synthetic corpus can only draft a synthetic certification"
    );
    assert_eq!(draft.profiles.keys().collect::<Vec<_>>(), ["core"]);
    let core = &draft.profiles["core"];
    assert_eq!(core.threshold, 2.5);
    // Signals seen in attacks, minus the tamper flag (which never uses a
    // ratio); rates floored to four decimals.
    assert_eq!(
        core.signals.keys().collect::<Vec<_>>(),
        ["app_switch", "cheat_tool_process", "paste_detected"]
    );
    assert_eq!(core.signals["app_switch"].precision, 0.6666);
    assert_eq!(core.signals["app_switch"].recall, 0.4);

    let file = copy_fixture().join("draft.json");
    std::fs::write(&file, &out).unwrap();
    let (outcome, out) = run(&["check", path(&fixture()), path(&file)]);
    assert_eq!(outcome.unwrap(), Outcome::Pass, "{out}");
}

fn entry_args<'a>(capture: &'a str, label: &'a str, provenance: &'a str) -> Vec<&'a str> {
    vec![
        "entry",
        "--capture",
        capture,
        "--id",
        "red-team-paste-01",
        "--label",
        label,
        "--provenance",
        provenance,
        "--profile",
        "core",
        "--platform",
        "macos",
        "--device-class",
        "desktop",
    ]
}

#[test]
fn a_captured_session_becomes_an_entry_the_corpus_accepts() {
    let dir = copy_fixture();
    let capture = dir.join("capture.jsonl");
    std::fs::write(
        &capture,
        concat!(
            r#"{"features":{"clipboard":{"paste_events":1,"pasted_chars":700}},"snapshot":{"id":"s1"}}"#,
            "\n",
            r#"{"backend_flags":["mcp_call_during_attempt"]}"#,
            "\n",
        ),
    )
    .unwrap();

    let refused = run(&entry_args(
        path(&capture),
        "paste_answer",
        "consented_clean",
    ))
    .0;
    assert!(
        refused.is_err(),
        "a consented clean session is always labelled clean"
    );

    let (outcome, out) = run(&entry_args(path(&capture), "paste_answer", "red_team"));
    assert_eq!(outcome.unwrap(), Outcome::Pass);
    std::fs::write(dir.join("entries/red-team-paste-01.json"), out).unwrap();
    let manifest_path = dir.join("manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
    manifest["entries"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({ "path": "entries/red-team-paste-01.json" }));
    std::fs::write(&manifest_path, manifest.to_string()).unwrap();
    run(&["seal", path(&manifest_path)]).0.unwrap();

    let corpus = Corpus::load(&manifest_path).unwrap();
    let core = &evaluate(&corpus, &RatioManifest::uncertified()).profiles["core"];
    assert_eq!(core.attack_sessions, 6);
    assert_eq!(core.signals["paste_detected"].fired_attack, 3);
    assert_eq!(core.signals["mcp_call_during_attempt"].fired_attack, 1);
}

#[test]
fn a_synthetic_certification_never_scores_sessions() {
    let (cert, _) = Certification::load(&certification()).unwrap();
    assert!(cert.session_manifest().is_err());
}

#[test]
fn a_certified_rate_the_corpus_no_longer_meets_fails_closed() {
    let dir = copy_fixture();
    let cert = dir.join("certification.json");
    let raised = std::fs::read_to_string(certification()).unwrap().replace(
        "\"app_switch\": { \"llr\": 0.6242, \"precision\": 0.6666, \"recall\": 0.4 }",
        "\"app_switch\": { \"llr\": 0.6242, \"precision\": 0.6666, \"recall\": 0.6 }",
    );
    std::fs::write(&cert, raised).unwrap();

    let (outcome, out) = run(&["check", path(&fixture()), path(&cert)]);
    assert_eq!(outcome.unwrap(), Outcome::Fail, "{out}");
    assert!(
        out.contains("FAIL core app_switch recall 0.400 < certified 0.600"),
        "{out}"
    );
}

#[test]
fn a_certified_signal_missing_from_the_corpus_fails_closed() {
    let dir = copy_fixture();
    let cert = dir.join("certification.json");
    let extra = std::fs::read_to_string(certification()).unwrap().replace(
        "\"signals\": {\n        \"app_switch\"",
        "\"signals\": {\n        \"hidden_overlay\": { \"llr\": 1.0, \"precision\": 0.9, \"recall\": 0.5 },\n        \"app_switch\"",
    );
    std::fs::write(&cert, extra).unwrap();

    let (outcome, out) = run(&["check", path(&fixture()), path(&cert)]);
    assert_eq!(outcome.unwrap(), Outcome::Fail, "{out}");
    assert!(
        out.contains("FAIL core hidden_overlay never fired"),
        "{out}"
    );
}

#[test]
fn a_certification_for_another_corpus_is_refused() {
    let dir = copy_fixture();
    std::fs::remove_file(dir.join("entries/clean-core-ios-01.json")).unwrap();
    let manifest = std::fs::read_to_string(dir.join("manifest.json")).unwrap();
    let start = manifest
        .find("    {\n      \"path\": \"entries/clean-core-ios-01.json\"")
        .unwrap();
    let end = start + manifest[start..].find("},\n").unwrap() + 3;
    std::fs::write(
        dir.join("manifest.json"),
        format!("{}{}", &manifest[..start], &manifest[end..]),
    )
    .unwrap();
    run(&["seal", path(&dir.join("manifest.json"))]).0.unwrap();

    let (outcome, _) = run(&[
        "check",
        path(&dir.join("manifest.json")),
        path(&certification()),
    ]);
    assert!(
        matches!(outcome, Err(MeasurementError::Invalid(ref m)) if m.contains("rests on corpus")),
        "{outcome:?}"
    );
}
