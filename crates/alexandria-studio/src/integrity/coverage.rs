//! Coverage statements: what a session under a trust profile and a
//! verification rung checked, what it did not, the platform ceilings, and
//! the error rates the profile was certified at.
//!
//! One statement per profile and rung, generated here from fixed text so the
//! app carries every one of them offline. A statement is content-addressed:
//! its digest is [`digest_bytes`] over [`CoverageStatement::bytes`], and the
//! integrity assertion carries that digest. When a certification is in
//! force, the statement embeds the certification's digest and the profile's
//! certified rates, so a new certification yields new statements.
//!
//! The text is the contract: changing a line changes every digest it is part
//! of. Bump [`COVERAGE_VERSION`] with any change so the two versions can be
//! told apart.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::measurement::{digest_bytes, Certification, CertifiedSignal};
use super::{ProfileSignal, TrustProfile, VerificationRung};

/// Version of the statement structure.
pub const COVERAGE_FORMAT: u32 = 1;
/// Version of the statement text.
pub const COVERAGE_VERSION: &str = "1";

/// The certification a statement's rates come from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CertificationRef {
    pub version: String,
    pub date: String,
    /// Digest of the certification manifest's exact bytes.
    pub digest: String,
    pub corpus_digest: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoverageStatement {
    pub format: u32,
    pub version: String,
    pub profile: TrustProfile,
    pub rung: VerificationRung,
    /// What a session under this profile and rung checked.
    pub checked: Vec<String>,
    /// What it did not check. Always says that a second person or a second
    /// device is invisible to monitoring.
    pub not_checked: Vec<String>,
    /// Limits set by the platform rather than the profile.
    pub platform_ceilings: Vec<String>,
    /// How the signals' weight was established.
    pub measurement: String,
    /// `None` until a certification is in force.
    pub certification: Option<CertificationRef>,
    /// The profile's certified signals and their rates; empty while every
    /// signal is advisory.
    pub certified_signals: BTreeMap<String, CertifiedSignal>,
}

impl CoverageStatement {
    /// The exact bytes the digest covers (compact JSON, field order fixed).
    pub fn bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("a coverage statement always serialises")
    }

    pub fn digest(&self) -> String {
        digest_bytes(&self.bytes())
    }
}

const CORE_CHECKED: &[&str] = &[
    "Typing, pointer, focus and clipboard behaviour in the assessment window.",
    "Display changes, hidden overlays, screen capture and watched processes, where the operating system reports them.",
    "Use of Alexandria's assistant, or of a connected external assistant, during the attempt, recorded by the backend; either suspends the session.",
];
const ATTESTATION_CHECKED: &str = "A platform attestation that an unmodified build of the app on a genuine device ran the session, verified again when the credential was issued.";
const CAMERA_CHECKED: &str = "Face presence, liveness and gaze from the device camera, at a cadence the device benchmark showed it can sustain. Windows without camera checks are recorded as degraded, never as clean.";

const NOT_ATTESTED: &str = "Whether the app or the device was modified: the session's figures are reported by the device itself.";
const NO_CAMERA: &str = "Who was at the device: the camera was not used.";
const ALWAYS_NOT_CHECKED: &[&str] = &[
    "A second person, in the room or helping remotely, or a second device out of view: monitoring cannot see either.",
    "Sound: nothing is recorded or transcribed.",
];

const RUNG_CHECKED: [(VerificationRung, &str); 4] = [
    (
        VerificationRung::Process,
        "Response timing and revisions on every served item, against reading-time floors and the learner's own pace, from items the backend drew by exposure and difficulty.",
    ),
    (
        VerificationRung::Probed,
        "Short follow-up questions made from the learner's own answers, answered within a time limit and graded by the item's own grader.",
    ),
    (
        VerificationRung::Consistent,
        "Agreement of the result with the learner's practice, lessons and earlier attempts on the skill.",
    ),
    (
        VerificationRung::Live,
        "An unscripted live segment with an interviewer in an interview held in Alexandria.",
    ),
];
const RUNG_NOT_CHECKED: [(VerificationRung, &str); 4] = [
    (
        VerificationRung::Process,
        "The response process over every served item: it was not observed in full, or showed a measured anomaly.",
    ),
    (
        VerificationRung::Probed,
        "Follow-up questions on the learner's own answers: none were satisfied.",
    ),
    (
        VerificationRung::Consistent,
        "Agreement with the learner's history on the skill: not established.",
    ),
    (
        VerificationRung::Live,
        "A live interview segment: none was satisfied.",
    ),
];

const PLATFORM_CEILINGS: &[&str] = &[
    "Device attestation exists only where the platform offers it (App Attest on iOS, Play Integrity on Android); elsewhere the assurance level stays local.",
    "Overlay, process and display checks see only what each operating system reports.",
];

const UNCERTIFIED: &str = "No signal has a certified error rate yet: every signal is advisory and can send a session to review, never decide it.";
const CERTIFIED: &str = "Signals listed with rates were measured on a labelled corpus of red-team and consented clean sessions; any other signal is advisory.";

/// The statement for `profile` and `rung`. `certification` is the
/// certification in force with its digest; a synthetic one is never
/// embedded, since no session is scored against it.
pub fn coverage_statement(
    profile: TrustProfile,
    rung: VerificationRung,
    certification: Option<(&Certification, &str)>,
) -> CoverageStatement {
    let mut checked: Vec<String> = CORE_CHECKED.iter().map(|s| s.to_string()).collect();
    let mut not_checked = Vec::new();
    if profile.includes(ProfileSignal::DeviceAttestation) {
        checked.push(ATTESTATION_CHECKED.into());
    } else {
        not_checked.push(NOT_ATTESTED.to_string());
    }
    if profile.includes(ProfileSignal::Camera) {
        checked.push(CAMERA_CHECKED.into());
    } else {
        not_checked.push(NO_CAMERA.to_string());
    }
    for (step, line) in RUNG_CHECKED {
        if step <= rung {
            checked.push(line.into());
        }
    }
    for (step, line) in RUNG_NOT_CHECKED {
        if step > rung {
            not_checked.push(line.into());
        }
    }
    not_checked.extend(ALWAYS_NOT_CHECKED.iter().map(|s| s.to_string()));

    let certification = certification.filter(|(c, _)| !c.synthetic);
    let certified_signals = certification
        .and_then(|(c, _)| c.profiles.get(profile.as_str()))
        .map(|p| p.signals.clone())
        .unwrap_or_default();
    CoverageStatement {
        format: COVERAGE_FORMAT,
        version: COVERAGE_VERSION.into(),
        profile,
        rung,
        checked,
        not_checked,
        platform_ceilings: PLATFORM_CEILINGS.iter().map(|s| s.to_string()).collect(),
        measurement: if certified_signals.is_empty() {
            UNCERTIFIED
        } else {
            CERTIFIED
        }
        .into(),
        certification: certification.map(|(c, digest)| CertificationRef {
            version: c.version.clone(),
            date: c.date.clone(),
            digest: digest.to_string(),
            corpus_digest: c.corpus_digest.clone(),
        }),
        certified_signals,
    }
}

/// Every statement under `certification`, one per profile and rung.
pub fn coverage_statements(
    certification: Option<(&Certification, &str)>,
) -> Vec<CoverageStatement> {
    TrustProfile::ALL
        .into_iter()
        .flat_map(|p| {
            VerificationRung::ALL
                .into_iter()
                .map(move |r| coverage_statement(p, r, certification))
        })
        .collect()
}

/// The statement with `digest` under `certification`, if there is one.
pub fn resolve_coverage(
    digest: &str,
    certification: Option<(&Certification, &str)>,
) -> Option<CoverageStatement> {
    coverage_statements(certification)
        .into_iter()
        .find(|s| s.digest() == digest)
}
