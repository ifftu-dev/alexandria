//! The verification rung: positive evidence that the learner did the work,
//! independent of the device.
//!
//! `none` → `process` → `probed` → `consistent` → `live`. The rungs form a
//! ladder: a session stands on the highest rung whose evidence holds along
//! with that of every rung below it, so a coverage statement for a rung can
//! state everything beneath it too. Evidence that is missing or skipped
//! (no probe policy, too little history) never counts against the learner;
//! it only leaves the session on the rung below.
//!
//! [`VerificationRung`] lives in the verifier library
//! ([`alexandria_verify::integrity`]), where an issuance policy's minimum
//! rung is judged; this module decides which rung a session reached.

use serde::{Deserialize, Serialize};

pub use alexandria_verify::integrity::VerificationRung;

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
