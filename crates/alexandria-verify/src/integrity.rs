//! The vocabulary of an integrity assertion: trust profiles, the signals they
//! add, verification rungs, and how a policy floor is judged against a
//! profile.
//!
//! A credential's [`IntegrityAssertion`](crate::vc::IntegrityAssertion)
//! names these as strings (`trustProfile`, `signalsRun`, `verificationRung`);
//! the types here give them their order and meaning, so the issuing app and
//! any verifier judge an issuance policy ([`crate::policy`]) the same way.
//! How a session comes to run a profile (selection, benchmarks, consent) is
//! the issuing app's business and is not here.
//!
//! Four profiles, ordered `core < attested < camera < full`. Every profile
//! runs the core signals. On top of those, `attested` needs a verified device
//! attestation, `camera` needs the camera, and `full` needs both. A floor is
//! met when the session's profile includes every signal the floor profile
//! adds. That is a signal-set test, not the order: `camera` does not meet an
//! `attested` floor.
//!
//! The verification rungs form a ladder, `none` → `process` → `probed` →
//! `consistent` → `live`: a session stands on the highest rung whose evidence
//! holds along with that of every rung below it.

use serde::{Deserialize, Serialize};

/// Camera coverage the `camera` and `full` profiles need for the camera to
/// count towards a policy floor: the fraction of committed windows that
/// carried camera checks (see [`TrustProfile::min_camera_coverage`]). An
/// assertion states it as `cameraCoverage`, so the issuing device and every
/// verifier judge it the same way ([`camera_coverage_counts`]).
pub const MIN_CAMERA_COVERAGE: f64 = 0.9;

/// Whether a session's camera coverage, the fraction of committed windows
/// that carried camera checks, lets the camera count towards a policy floor:
/// at least [`MIN_CAMERA_COVERAGE`] and no more than 1. A value outside
/// `[0,1]` is malformed and never counts.
pub fn camera_coverage_counts(coverage: f64) -> bool {
    (MIN_CAMERA_COVERAGE..=1.0).contains(&coverage)
}

/// A fixed set of signals a session runs, locked at session start.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrustProfile {
    Core,
    Attested,
    Camera,
    Full,
}

/// A signal a profile adds to the core set.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileSignal {
    /// A platform attestation of the session (App Attest or Play Integrity)
    /// whose evidence verifies, credential- or session-bound.
    DeviceAttestation,
    /// Face presence, liveness and gaze from the camera.
    Camera,
}

impl TrustProfile {
    /// Weakest first.
    pub const ALL: [TrustProfile; 4] = [
        TrustProfile::Core,
        TrustProfile::Attested,
        TrustProfile::Camera,
        TrustProfile::Full,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            TrustProfile::Core => "core",
            TrustProfile::Attested => "attested",
            TrustProfile::Camera => "camera",
            TrustProfile::Full => "full",
        }
    }

    pub fn parse(profile: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.as_str() == profile)
    }

    /// The signals this profile adds to the core set.
    pub fn signals(self) -> &'static [ProfileSignal] {
        match self {
            TrustProfile::Core => &[],
            TrustProfile::Attested => &[ProfileSignal::DeviceAttestation],
            TrustProfile::Camera => &[ProfileSignal::Camera],
            TrustProfile::Full => &[ProfileSignal::DeviceAttestation, ProfileSignal::Camera],
        }
    }

    /// The fraction of committed windows that must carry camera checks for
    /// the camera to count towards a policy floor, on the issuing device and
    /// for any verifier reading the assertion's `cameraCoverage`. A property
    /// of the camera profiles, not a sponsor knob; an operating constant
    /// until measured.
    pub fn min_camera_coverage(self) -> Option<f64> {
        self.includes(ProfileSignal::Camera)
            .then_some(MIN_CAMERA_COVERAGE)
    }

    pub fn includes(self, signal: ProfileSignal) -> bool {
        self.signals().contains(&signal)
    }

    /// Whether a session under this profile runs every signal `floor` adds.
    pub fn meets(self, floor: TrustProfile) -> bool {
        floor.signals().iter().all(|s| self.includes(*s))
    }
}

impl ProfileSignal {
    pub const ALL: [ProfileSignal; 2] = [ProfileSignal::DeviceAttestation, ProfileSignal::Camera];

    pub fn as_str(self) -> &'static str {
        match self {
            ProfileSignal::DeviceAttestation => "device_attestation",
            ProfileSignal::Camera => "camera",
        }
    }

    pub fn parse(signal: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.as_str() == signal)
    }
}

/// A policy floor judged against a session.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyFloor {
    /// The minimum profile the policy asks for; `None` asks for nothing.
    pub required: Option<TrustProfile>,
    /// The session's locked profile; `None` when none was selected.
    pub profile: Option<TrustProfile>,
    pub satisfied: bool,
    /// Signals the floor needs that the session does not run or that are no
    /// longer credited (an attestation that stopped verifying).
    pub missing: Vec<ProfileSignal>,
}

/// Judge `floor` against a session locked to `profile`. `uncredited` lists
/// the signals the profile runs that do not count at the time of the
/// evaluation: an attestation that does not verify, or a camera that was
/// lost. A profile cannot carry a floor on a signal it has lost.
pub fn evaluate_floor(
    profile: Option<TrustProfile>,
    floor: Option<TrustProfile>,
    uncredited: &[ProfileSignal],
) -> PolicyFloor {
    let missing: Vec<ProfileSignal> = floor
        .map(|f| f.signals())
        .unwrap_or_default()
        .iter()
        .copied()
        .filter(|s| {
            let runs = profile.is_some_and(|p| p.includes(*s));
            !runs || uncredited.contains(s)
        })
        .collect();
    // With a floor, a session that never selected a profile cannot meet it,
    // even a `core` floor: capability absence is never clean.
    let satisfied = match floor {
        None => true,
        Some(_) => profile.is_some() && missing.is_empty(),
    };
    PolicyFloor {
        required: floor,
        profile,
        satisfied,
        missing,
    }
}

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
