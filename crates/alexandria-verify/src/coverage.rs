//! Coverage statements: what an integrity session under a trust profile and
//! a verification rung checked, what it did not, the platform ceilings, and
//! the error rates the profile was certified at.
//!
//! This is the registry of every statement text ever published. A statement
//! is content-addressed: its digest is `blake3:` over
//! [`CoverageStatement::bytes`], and a credential's integrity assertion
//! carries that digest (`coverageStatementDigest`). Because the digest is
//! signed into credentials, a published text is never changed or removed:
//! a text change adds a version to [`VERSIONS`], and [`resolve`] finds a
//! statement of any version from its digest, offline.
//!
//! A credential also names its statement by the standard `relatedResource`
//! property ([`related_resource`]): the statement's [`address`] and a
//! Subresource Integrity digest of the same bytes. The address embeds the
//! digest, so the verifier rejects a credential whose `relatedResource`
//! names a statement other than its `coverageStatementDigest`
//! ([`names_mismatched_statement`]) without resolving either.
//!
//! Statement lines are English in every locale and are quoted verbatim:
//! translating or rewording one would change what the digest covers.
//!
//! When a certification is in force the statement embeds the certification's
//! digest and the profile's certified rates, so a new certification yields
//! new statements. Those resolve only when the caller supplies the same
//! certification ([`Certified`]); statements without one resolve from this
//! library alone.
//!
//! Version history:
//! - `1`: the first published text.
//! - `2`: the attestation line says that the issuing device verified the
//!   attestation, and that a reader relies on it only as far as the
//!   credential's attestation evidence verifies.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::vc::{RelatedResource, VerifiableCredential};

/// Version of the statement structure.
pub const COVERAGE_FORMAT: u32 = 1;
/// The statement text version new sessions are issued against.
pub const CURRENT_VERSION: &str = "2";
/// Every statement text version ever published, oldest first.
pub const VERSIONS: [&str; 2] = ["1", "2"];
/// Trust profiles a statement can be about, weakest first.
pub const PROFILES: [&str; 4] = ["core", "attested", "camera", "full"];
/// Verification rungs a statement can be about, lowest first.
pub const RUNGS: [&str; 5] = ["none", "process", "probed", "consistent", "live"];

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

/// A signal's certified rates.
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

/// A certification in force, as statements embed it: its reference and each
/// profile's certified signals, keyed by profile name. Only a reviewed,
/// non-synthetic certification is ever passed here.
#[derive(Debug, Clone, PartialEq)]
pub struct Certified {
    pub reference: CertificationRef,
    pub profiles: BTreeMap<String, BTreeMap<String, CertifiedSignal>>,
}

/// One coverage statement. Field order is part of the digest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoverageStatement {
    pub format: u32,
    /// Text version, one of [`VERSIONS`].
    pub version: String,
    /// Trust profile, one of [`PROFILES`].
    pub profile: String,
    /// Verification rung, one of [`RUNGS`].
    pub rung: String,
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

    /// `blake3:` and the hex BLAKE3 hash of [`Self::bytes`].
    pub fn digest(&self) -> String {
        format!("blake3:{}", blake3::hash(&self.bytes()).to_hex())
    }
}

// ---------------------------------------------------------------------------
// The text. Lines shared by every version are written once; a line that a
// version rewords lives in that version's `Text`.
// ---------------------------------------------------------------------------

/// The lines that differ between versions.
struct Text {
    attestation_checked: &'static str,
}

const TEXT_V1: Text = Text {
    attestation_checked: "A platform attestation that an unmodified build of the app on a genuine device ran the session, verified again when the credential was issued.",
};
const TEXT_V2: Text = Text {
    attestation_checked: "A platform attestation that an unmodified build of the app on a genuine device ran the session, verified by the issuing device when the credential was issued. A reader relies on it only as far as the credential's attestation evidence verifies.",
};

fn text(version: &str) -> Option<&'static Text> {
    match version {
        "1" => Some(&TEXT_V1),
        "2" => Some(&TEXT_V2),
        _ => None,
    }
}

const CORE_CHECKED: &[&str] = &[
    "Typing, pointer, focus and clipboard behaviour in the assessment window.",
    "Display changes, hidden overlays, screen capture and watched processes, where the operating system reports them.",
    "Use of Alexandria's assistant, or of a connected external assistant, during the attempt, recorded by the backend; either suspends the session.",
];
const CAMERA_CHECKED: &str = "Face presence, liveness and gaze from the device camera, at a cadence the device benchmark showed it can sustain. Windows without camera checks are recorded as degraded, never as clean.";

const NOT_ATTESTED: &str = "Whether the app or the device was modified: the session's figures are reported by the device itself.";
const NO_CAMERA: &str = "Who was at the device: the camera was not used.";
const ALWAYS_NOT_CHECKED: &[&str] = &[
    "A second person, in the room or helping remotely, or a second device out of view: monitoring cannot see either.",
    "Sound: nothing is recorded or transcribed.",
];

/// Indexed by rung position in [`RUNGS`], from `process` up.
const RUNG_CHECKED: [&str; 4] = [
    "Response timing and revisions on every served item, against reading-time floors and the learner's own pace, from items the backend drew by exposure and difficulty.",
    "Short follow-up questions made from the learner's own answers, answered within a time limit and graded by the item's own grader.",
    "Agreement of the result with the learner's practice, lessons and earlier attempts on the skill.",
    "An unscripted live segment with an interviewer in an interview held in Alexandria.",
];
const RUNG_NOT_CHECKED: [&str; 4] = [
    "The response process over every served item: it was not observed in full, or showed a measured anomaly.",
    "Follow-up questions on the learner's own answers: none were satisfied.",
    "Agreement with the learner's history on the skill: not established.",
    "A live interview segment: none was satisfied.",
];

const PLATFORM_CEILINGS: &[&str] = &[
    "Device attestation exists only where the platform offers it (App Attest on iOS, Play Integrity on Android); elsewhere the assurance level stays local.",
    "Overlay, process and display checks see only what each operating system reports.",
];

const UNCERTIFIED: &str = "No signal has a certified error rate yet: every signal is advisory and can send a session to review, never decide it.";
const CERTIFIED: &str = "Signals listed with rates were measured on a labelled corpus of red-team and consented clean sessions; any other signal is advisory.";

/// The statement of text `version` for `profile` and `rung`, embedding
/// `certification` when one is in force. `None` for an unknown version,
/// profile or rung.
pub fn statement(
    version: &str,
    profile: &str,
    rung: &str,
    certification: Option<&Certified>,
) -> Option<CoverageStatement> {
    let text = text(version)?;
    if !PROFILES.contains(&profile) {
        return None;
    }
    let rung_step = RUNGS.iter().position(|r| *r == rung)?;
    let attested = matches!(profile, "attested" | "full");
    let camera = matches!(profile, "camera" | "full");

    let mut checked: Vec<String> = CORE_CHECKED.iter().map(|s| s.to_string()).collect();
    let mut not_checked = Vec::new();
    if attested {
        checked.push(text.attestation_checked.into());
    } else {
        not_checked.push(NOT_ATTESTED.to_string());
    }
    if camera {
        checked.push(CAMERA_CHECKED.into());
    } else {
        not_checked.push(NO_CAMERA.to_string());
    }
    for (i, line) in RUNG_CHECKED.iter().enumerate() {
        if i < rung_step {
            checked.push(line.to_string());
        }
    }
    for (i, line) in RUNG_NOT_CHECKED.iter().enumerate() {
        if i >= rung_step {
            not_checked.push(line.to_string());
        }
    }
    not_checked.extend(ALWAYS_NOT_CHECKED.iter().map(|s| s.to_string()));

    let certified_signals = certification
        .and_then(|c| c.profiles.get(profile))
        .cloned()
        .unwrap_or_default();
    Some(CoverageStatement {
        format: COVERAGE_FORMAT,
        version: version.into(),
        profile: profile.into(),
        rung: rung.into(),
        checked,
        not_checked,
        platform_ceilings: PLATFORM_CEILINGS.iter().map(|s| s.to_string()).collect(),
        measurement: if certified_signals.is_empty() {
            UNCERTIFIED
        } else {
            CERTIFIED
        }
        .into(),
        certification: certification.map(|c| c.reference.clone()),
        certified_signals,
    })
}

/// Every statement of text `version`, one per profile and rung.
pub fn statements(version: &str, certification: Option<&Certified>) -> Vec<CoverageStatement> {
    PROFILES
        .into_iter()
        .flat_map(|p| RUNGS.into_iter().map(move |r| (p, r)))
        .filter_map(|(p, r)| statement(version, p, r, certification))
        .collect()
}

/// Prefix of a statement address when the issuer has no status host.
pub const URN_PREFIX: &str = "urn:alexandria:coverage-statement:";
/// Path under a status host that serves statements by digest.
pub const HOST_PATH: &str = "/coverage-statements/";

/// Where the statement with `digest` is named: `{host}/coverage-statements/
/// {digest}` when the issuer's profile has a status host, else
/// `urn:alexandria:coverage-statement:{digest}`.
pub fn address(digest: &str, host: Option<&str>) -> String {
    match host {
        Some(host) => format!("{}{HOST_PATH}{digest}", host.trim_end_matches('/')),
        None => format!("{URN_PREFIX}{digest}"),
    }
}

/// The statement digest an address names, if `id` is a statement address in
/// either form.
pub fn digest_in_address(id: &str) -> Option<&str> {
    let digest = match id.strip_prefix(URN_PREFIX) {
        Some(digest) => digest,
        None => {
            let (origin, digest) = id.rsplit_once(HOST_PATH)?;
            if !origin.contains("://") {
                return None;
            }
            digest
        }
    };
    (!digest.is_empty() && !digest.contains('/')).then_some(digest)
}

/// The `relatedResource` entry that names `statement`: its [`address`] and a
/// Subresource Integrity SHA-256 digest of its exact bytes.
pub fn related_resource(statement: &CoverageStatement, host: Option<&str>) -> RelatedResource {
    use base64::Engine as _;
    let sha256 = crate::hash::sha256(&statement.bytes());
    RelatedResource {
        id: address(&statement.digest(), host),
        digest_sri: Some(format!(
            "sha256-{}",
            base64::engine::general_purpose::STANDARD.encode(sha256)
        )),
        digest_multibase: None,
        media_type: Some("application/json".into()),
    }
}

/// Whether `credential` names a coverage statement by `relatedResource` that
/// is not the one its integrity assertion names by digest.
pub fn names_mismatched_statement(credential: &VerifiableCredential) -> bool {
    let named = credential
        .integrity
        .as_ref()
        .and_then(|a| a.coverage_statement_digest.as_deref());
    credential
        .related_resource
        .iter()
        .filter_map(|r| digest_in_address(&r.id))
        .any(|digest| Some(digest) != named)
}

/// The statement of any published version with `digest`. Statements without
/// a certification always resolve; certified ones resolve when
/// `certification` is the one they embed.
pub fn resolve(digest: &str, certification: Option<&Certified>) -> Option<CoverageStatement> {
    let mut candidates: Vec<Option<&Certified>> = vec![None];
    if certification.is_some() {
        candidates.push(certification);
    }
    candidates.into_iter().find_map(|c| {
        VERSIONS
            .into_iter()
            .flat_map(|v| statements(v, c))
            .find(|s| s.digest() == digest)
    })
}
