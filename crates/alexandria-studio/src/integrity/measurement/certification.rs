//! Certification: the reviewed manifest that admits signals to a trust
//! profile with the rates they were measured at, and the fail-closed check
//! that a corpus replay still meets them.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::corpus::{digest_bytes, parse};
use super::{MeasurementError, Report};
use crate::integrity::{is_tamper_flag, ProfileRatios, RatioManifest};

/// Version of the certification manifest format.
pub const CERTIFICATION_FORMAT: u32 = 1;

/// Rates are compared after this much slack, so a rate printed and committed
/// at four decimals still matches the replay it came from.
const RATE_EPSILON: f64 = 1e-9;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CertifiedSignal {
    /// Log-likelihood ratio the fusion adds each window the signal fires.
    pub llr: f64,
    /// Session-level precision measured at certification; the floor CI holds.
    pub precision: f64,
    /// Session-level recall measured at certification; the floor CI holds.
    pub recall: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CertifiedProfile {
    /// Operating threshold: accumulated channel LLR that counts as over.
    pub threshold: f64,
    /// The admitted signal set. A signal not listed stays advisory.
    pub signals: BTreeMap<String, CertifiedSignal>,
}

/// A reviewed certification manifest. Content-addressed by the digest of its
/// exact file bytes ([`Certification::load`]); coverage statements embed
/// that digest. Changing a threshold or a signal set means a new one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Certification {
    pub format: u32,
    pub version: String,
    /// ISO date the measurement was reviewed.
    pub date: String,
    /// Built from a synthetic corpus: a contract fixture that no session is
    /// ever scored against.
    pub synthetic: bool,
    pub corpus_id: String,
    pub corpus_digest: String,
    pub profiles: BTreeMap<String, CertifiedProfile>,
}

/// A certified rate the replay no longer meets.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Shortfall {
    pub profile: String,
    pub signal: String,
    /// `precision` or `recall`, or `fired` when the signal never fired.
    pub metric: &'static str,
    pub certified: f64,
    pub measured: Option<f64>,
}

impl std::fmt::Display for Shortfall {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (profile, signal) = (&self.profile, &self.signal);
        match self.measured {
            Some(measured) => write!(
                f,
                "FAIL {profile} {signal} {} {measured:.3} < certified {:.3}",
                self.metric, self.certified
            ),
            None => write!(f, "FAIL {profile} {signal} never fired in this corpus"),
        }
    }
}

fn rate_ok(value: f64) -> bool {
    value.is_finite() && (0.0..=1.0).contains(&value)
}

impl Certification {
    /// Reads a certification and returns it with its digest.
    pub fn load(path: &Path) -> Result<(Self, String), MeasurementError> {
        let bytes = std::fs::read(path).map_err(|source| MeasurementError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let certification: Self = parse(path, &bytes)?;
        certification
            .validate()
            .map_err(MeasurementError::Invalid)?;
        Ok((certification, digest_bytes(&bytes)))
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.format != CERTIFICATION_FORMAT {
            return Err(format!(
                "certification format {} is not supported (expected {CERTIFICATION_FORMAT})",
                self.format
            ));
        }
        if self.version.trim().is_empty() || self.date.trim().is_empty() {
            return Err("certification needs a version and a date".into());
        }
        self.ratio_manifest().validate()?;
        for (profile, p) in &self.profiles {
            for (signal, s) in &p.signals {
                if !rate_ok(s.precision) || !rate_ok(s.recall) {
                    return Err(format!(
                        "{profile} {signal}: rates must be between zero and one"
                    ));
                }
            }
        }
        Ok(())
    }

    /// A starting point for review: for each profile given a threshold,
    /// every signal that fired in an attack session with a positive
    /// likelihood ratio, at its measured rates floored to four decimals.
    /// Tamper flags are left out (they suspend without a ratio). Reviewers
    /// prune the signal set; a corpus with synthetic entries can only draft
    /// a synthetic certification.
    pub fn draft(
        report: &Report,
        version: &str,
        date: &str,
        thresholds: &BTreeMap<String, f64>,
    ) -> Self {
        let floor4 = |v: f64| (v * 10_000.0).floor() / 10_000.0;
        let round4 = |v: f64| (v * 10_000.0).round() / 10_000.0;
        let profiles = thresholds
            .iter()
            .filter_map(|(name, threshold)| {
                let measured = report.profiles.get(name)?;
                let signals = measured
                    .signals
                    .iter()
                    .filter(|(signal, m)| {
                        !is_tamper_flag(signal) && m.fired_attack > 0 && m.llr.unwrap_or(0.0) > 0.0
                    })
                    .map(|(signal, m)| {
                        let certified = CertifiedSignal {
                            llr: round4(m.llr.unwrap_or(0.0)),
                            precision: floor4(m.precision.unwrap_or(0.0)),
                            recall: floor4(m.recall.unwrap_or(0.0)),
                        };
                        (signal.clone(), certified)
                    })
                    .collect();
                Some((
                    name.clone(),
                    CertifiedProfile {
                        threshold: *threshold,
                        signals,
                    },
                ))
            })
            .collect();
        Self {
            format: CERTIFICATION_FORMAT,
            version: version.into(),
            date: date.into(),
            synthetic: report.synthetic,
            corpus_id: report.corpus_id.clone(),
            corpus_digest: report.corpus_digest.clone(),
            profiles,
        }
    }

    /// The ratios this certification admits, for replaying a corpus.
    pub fn ratio_manifest(&self) -> RatioManifest {
        RatioManifest {
            version: self.version.clone(),
            profiles: self
                .profiles
                .iter()
                .map(|(name, p)| {
                    let ratios = p.signals.iter().map(|(s, c)| (s.clone(), c.llr)).collect();
                    (
                        name.clone(),
                        ProfileRatios {
                            threshold: p.threshold,
                            ratios,
                        },
                    )
                })
                .collect(),
        }
    }

    /// The ratios live sessions are scored against. A synthetic
    /// certification is refused: synthetic data never decides a session.
    pub fn session_manifest(&self) -> Result<RatioManifest, String> {
        if self.synthetic {
            return Err(format!(
                "certification {} is synthetic and cannot score sessions",
                self.version
            ));
        }
        Ok(self.ratio_manifest())
    }

    /// Every certified rate the replay in `report` no longer meets. Refuses
    /// a report of another corpus, and a non-synthetic certification resting
    /// on a corpus with synthetic entries.
    pub fn check(&self, report: &Report) -> Result<Vec<Shortfall>, MeasurementError> {
        if report.corpus_digest != self.corpus_digest {
            return Err(MeasurementError::Invalid(format!(
                "certification {} rests on corpus {} {}, not {} {}",
                self.version,
                self.corpus_id,
                self.corpus_digest,
                report.corpus_id,
                report.corpus_digest
            )));
        }
        if report.synthetic && !self.synthetic {
            return Err(MeasurementError::Invalid(format!(
                "certification {} claims real measurement but its corpus has synthetic entries",
                self.version
            )));
        }
        let mut shortfalls = Vec::new();
        for (profile, certified) in &self.profiles {
            let measured = report.profiles.get(profile);
            for (signal, rates) in &certified.signals {
                let shortfall = |metric, certified, measured| Shortfall {
                    profile: profile.clone(),
                    signal: signal.clone(),
                    metric,
                    certified,
                    measured,
                };
                let Some(m) = measured.and_then(|p| p.signals.get(signal)) else {
                    shortfalls.push(shortfall("fired", rates.recall, None));
                    continue;
                };
                for (metric, floor, value) in [
                    ("precision", rates.precision, m.precision),
                    ("recall", rates.recall, m.recall),
                ] {
                    let value = value.unwrap_or(0.0);
                    if value + RATE_EPSILON < floor {
                        shortfalls.push(shortfall(metric, floor, Some(value)));
                    }
                }
            }
        }
        Ok(shortfalls)
    }
}
