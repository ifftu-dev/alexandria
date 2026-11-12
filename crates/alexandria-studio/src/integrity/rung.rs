//! The verification rung: positive evidence that the learner did the work,
//! independent of the device.
//!
//! `none` → `process` → `probed` → `consistent` → `live`. The rungs form a
//! ladder: a session stands on the highest rung whose evidence holds along
//! with that of every rung below it, so a coverage statement for a rung can
//! state everything beneath it too. Evidence that is missing or skipped
//! (no probe policy, too little history) never counts against the learner;
//! it only leaves the session on the rung below.

use serde::{Deserialize, Serialize};

/// How far up the verification axis a session reached.
#[derive(
    Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum VerificationRung {
    #[default]
    None,
    /// Response-process signals ran over every served item without a
    /// measured anomaly, and the items came from the backend's draw.
    Process,
    /// The probe policy ran and every probe was satisfied.
    Probed,
    /// The outcome agrees with the learner's own history on the skill.
    Consistent,
    /// An unscripted human segment in an Alexandria-hosted interview was
    /// satisfied.
    Live,
}

impl VerificationRung {
    /// Lowest first.
    pub const ALL: [VerificationRung; 5] = [
        VerificationRung::None,
        VerificationRung::Process,
        VerificationRung::Probed,
        VerificationRung::Consistent,
        VerificationRung::Live,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            VerificationRung::None => "none",
            VerificationRung::Process => "process",
            VerificationRung::Probed => "probed",
            VerificationRung::Consistent => "consistent",
            VerificationRung::Live => "live",
        }
    }

    pub fn parse(rung: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.as_str() == rung)
    }
}

/// Whether each rung's own evidence holds, judged on its own.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RungEvidence {
    pub process: bool,
    pub probed: bool,
    pub consistent: bool,
    pub live: bool,
}

/// The highest rung whose evidence holds together with every rung below.
pub fn verification_rung(evidence: &RungEvidence) -> VerificationRung {
    let steps = [
        (evidence.process, VerificationRung::Process),
        (evidence.probed, VerificationRung::Probed),
        (evidence.consistent, VerificationRung::Consistent),
        (evidence.live, VerificationRung::Live),
    ];
    steps
        .into_iter()
        .take_while(|(holds, _)| *holds)
        .last()
        .map_or(VerificationRung::None, |(_, rung)| rung)
}
