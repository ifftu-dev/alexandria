//! Replaying a corpus through the scoring code and measuring each signal.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use serde::Serialize;

use super::corpus::{Corpus, CLEAN_LABEL};
use crate::integrity::{decide, RatioManifest, SessionStatus};

/// How one signal behaved on one profile's sessions.
///
/// Session-level rates answer "does the signal catch attacks, and how often
/// is it wrong": a session counts once if the signal fired in any window.
/// The likelihood ratio is per window, because that is what the fusion adds
/// each window a signal fires (see [`RatioManifest`]).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct SignalMetrics {
    /// Attack sessions in which the signal fired at least once.
    pub fired_attack: usize,
    /// Clean sessions in which it fired at least once.
    pub fired_clean: usize,
    /// `fired_attack / (fired_attack + fired_clean)`; `None` if it never fired.
    pub precision: Option<f64>,
    /// `fired_attack / attack sessions`; `None` without attack sessions.
    pub recall: Option<f64>,
    /// `fired_clean / clean sessions`; `None` without clean sessions.
    pub false_positive_rate: Option<f64>,
    pub attack_windows_fired: usize,
    pub clean_windows_fired: usize,
    /// Natural log of the per-window firing rate in attack sessions over the
    /// rate in clean sessions, each with a 0.5 continuity correction
    /// (`(fired + 0.5) / (windows + 1)`) so a signal that never fired on one
    /// side still gets a finite ratio. `None` when either side has no windows.
    pub llr: Option<f64>,
    /// The ratio manifest the report ran against has a ratio for this signal.
    pub admitted: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ProfileReport {
    pub attack_sessions: usize,
    pub clean_sessions: usize,
    pub attack_windows: usize,
    pub clean_windows: usize,
    pub signals: BTreeMap<String, SignalMetrics>,
    /// Session outcome per ground-truth label: label → status → sessions.
    pub confusion: BTreeMap<String, BTreeMap<String, usize>>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Report {
    pub corpus_id: String,
    pub corpus_digest: String,
    pub synthetic: bool,
    pub manifest_version: String,
    pub profiles: BTreeMap<String, ProfileReport>,
}

fn ratio(numerator: usize, denominator: usize) -> Option<f64> {
    (denominator > 0).then(|| numerator as f64 / denominator as f64)
}

/// Replays every entry: per-window flags through [`crate::integrity::derive_window`],
/// the session through [`decide`] under `manifest` and the entry's profile.
pub fn evaluate(corpus: &Corpus, manifest: &RatioManifest) -> Report {
    let mut profiles: BTreeMap<String, ProfileReport> = BTreeMap::new();
    for entry in &corpus.entries {
        let evidence = entry.evidence();
        let clean = entry.is_clean();
        let report = profiles.entry(entry.profile.clone()).or_default();
        if clean {
            report.clean_sessions += 1;
            report.clean_windows += evidence.windows.len();
        } else {
            report.attack_sessions += 1;
            report.attack_windows += evidence.windows.len();
        }
        let mut fired_in_session: Vec<&str> = Vec::new();
        for flag in evidence.windows.iter().flatten() {
            let metrics = report.signals.entry(flag.clone()).or_default();
            if clean {
                metrics.clean_windows_fired += 1;
            } else {
                metrics.attack_windows_fired += 1;
            }
            if !fired_in_session.contains(&flag.as_str()) {
                fired_in_session.push(flag);
                if clean {
                    metrics.fired_clean += 1;
                } else {
                    metrics.fired_attack += 1;
                }
            }
        }
        let status = decide(&evidence, manifest, &entry.profile, true).status;
        let label = if clean { CLEAN_LABEL } else { &entry.label };
        *report
            .confusion
            .entry(label.to_string())
            .or_default()
            .entry(status.as_str().to_string())
            .or_default() += 1;
    }
    for (name, report) in &mut profiles {
        let admitted = manifest.profiles.get(name);
        for (signal, m) in &mut report.signals {
            m.precision = ratio(m.fired_attack, m.fired_attack + m.fired_clean);
            m.recall = ratio(m.fired_attack, report.attack_sessions);
            m.false_positive_rate = ratio(m.fired_clean, report.clean_sessions);
            m.llr = (report.attack_windows > 0 && report.clean_windows > 0).then(|| {
                let attack =
                    (m.attack_windows_fired as f64 + 0.5) / (report.attack_windows as f64 + 1.0);
                let clean =
                    (m.clean_windows_fired as f64 + 0.5) / (report.clean_windows as f64 + 1.0);
                (attack / clean).ln()
            });
            m.admitted = admitted.is_some_and(|p| p.ratios.contains_key(signal));
        }
    }
    Report {
        corpus_id: corpus.manifest.corpus_id.clone(),
        corpus_digest: corpus.digest().to_string(),
        synthetic: corpus.is_synthetic(),
        manifest_version: manifest.version.clone(),
        profiles,
    }
}

const STATUSES: [SessionStatus; 4] = [
    SessionStatus::Completed,
    SessionStatus::Review,
    SessionStatus::Flagged,
    SessionStatus::Suspended,
];

fn rate(value: Option<f64>) -> String {
    value.map_or_else(|| "-".into(), |v| format!("{v:.3}"))
}

impl Report {
    /// The plain-text report the evaluator prints.
    pub fn render(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "corpus {} {}", self.corpus_id, self.corpus_digest);
        if self.synthetic {
            let _ = writeln!(
                out,
                "SYNTHETIC corpus: contract tests only, never certification"
            );
        }
        let _ = writeln!(out, "ratio manifest {}", self.manifest_version);
        for (name, p) in &self.profiles {
            let _ = writeln!(
                out,
                "\nprofile {name}: {} attack / {} clean sessions, {} / {} windows",
                p.attack_sessions, p.clean_sessions, p.attack_windows, p.clean_windows
            );
            let _ = writeln!(
                out,
                "  {:<34} {:>9} {:>9} {:>9} {:>8}  status",
                "signal", "precision", "recall", "fpr", "llr"
            );
            for (signal, m) in &p.signals {
                let _ = writeln!(
                    out,
                    "  {:<34} {:>9} {:>9} {:>9} {:>8}  {}",
                    signal,
                    rate(m.precision),
                    rate(m.recall),
                    rate(m.false_positive_rate),
                    rate(m.llr),
                    if m.admitted { "admitted" } else { "advisory" }
                );
            }
            let _ = write!(out, "  fusion confusion {:<17}", "(label \\ status)");
            for status in STATUSES {
                let _ = write!(out, " {:>9}", status.as_str());
            }
            let _ = writeln!(out);
            for (label, row) in &p.confusion {
                let _ = write!(out, "  {:<34}", label);
                for status in STATUSES {
                    let n = row.get(status.as_str()).copied().unwrap_or(0);
                    let _ = write!(out, " {n:>9}");
                }
                let _ = writeln!(out);
            }
        }
        out
    }
}
