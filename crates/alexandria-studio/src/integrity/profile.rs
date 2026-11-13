//! Trust profiles: the fixed signal sets a session can run, how the backend
//! chooses one at session start, and how a policy floor is judged against it.
//!
//! Four profiles, ordered `core < attested < camera < full`. Every profile
//! runs the core signals (input, focus, clipboard, display, environment
//! probes, response process). On top of those, `attested` needs a verified
//! device attestation, `camera` needs the camera, and `full` needs both.
//!
//! Selection takes, in order: the attestation result, the capability report,
//! the device benchmark against each camera profile's budget, then the policy
//! floor and the learner's per-capability consent. A profile is permitted
//! only when every signal it adds is available, consented and affordable;
//! capability absence is never treated as clean. The chosen profile is the
//! highest permitted one that meets the floor, or the highest permitted one
//! when none does (the floor result then says so).
//!
//! A floor is met when the session's profile includes every signal the floor
//! profile adds. That is a signal-set test, not the order: `camera` does not
//! meet an `attested` floor.

use serde::{Deserialize, Serialize};

/// Camera coverage the `camera` and `full` profiles need for the camera to
/// count (see [`TrustProfile::min_camera_coverage`]).
pub const MIN_CAMERA_COVERAGE: f64 = 0.9;

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
    /// A platform attestation that verifies (`device_attested`).
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
    /// the camera to count towards a policy floor. A property of the camera
    /// profiles, not a sponsor knob; an operating constant until measured.
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
}

/// One flag per capability. Used for both what the device reports it has
/// and what the learner consented to for this session.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Capabilities {
    pub camera: bool,
    pub microphone: bool,
    pub companion_phone: bool,
}

/// Which camera profiles' budgets the device benchmark fitted.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BudgetFit {
    pub camera: bool,
    pub full: bool,
}

/// Everything selection looks at.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectionInputs {
    /// The session's device attestation verified.
    pub device_attested: bool,
    pub capabilities: Capabilities,
    pub consent: Capabilities,
    /// `None` when the benchmark did not run or failed.
    pub budget: Option<BudgetFit>,
    /// The minimum profile the governing policy asks for, if any.
    pub floor: Option<TrustProfile>,
}

/// Why a profile could not be selected.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExclusionReason {
    /// No device attestation verified for the session.
    NotAttested,
    /// The device reported no usable camera.
    NoCamera,
    /// The learner did not consent to the camera for this session.
    CameraNotConsented,
    /// The device benchmark did not run or did not finish.
    NotBenchmarked,
    /// The device benchmark exceeded this profile's budget.
    OverBudget,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Exclusion {
    pub profile: TrustProfile,
    pub reasons: Vec<ExclusionReason>,
}

/// The backend's choice and why.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileSelection {
    pub profile: TrustProfile,
    pub inputs: SelectionInputs,
    /// Every profile that was not permitted, with its reasons.
    pub excluded: Vec<Exclusion>,
    /// Signals the chosen profile does not run ("not run" in the assertion).
    pub not_run: Vec<ProfileSignal>,
}

/// Why `profile` is not permitted under `inputs`; empty when it is.
pub fn exclusion_reasons(inputs: &SelectionInputs, profile: TrustProfile) -> Vec<ExclusionReason> {
    let mut reasons = Vec::new();
    if profile.includes(ProfileSignal::DeviceAttestation) && !inputs.device_attested {
        reasons.push(ExclusionReason::NotAttested);
    }
    if profile.includes(ProfileSignal::Camera) {
        if !inputs.capabilities.camera {
            reasons.push(ExclusionReason::NoCamera);
        } else if !inputs.consent.camera {
            reasons.push(ExclusionReason::CameraNotConsented);
        } else {
            match inputs.budget {
                None => reasons.push(ExclusionReason::NotBenchmarked),
                Some(fit) => {
                    let fits = match profile {
                        TrustProfile::Full => fit.full,
                        _ => fit.camera,
                    };
                    if !fits {
                        reasons.push(ExclusionReason::OverBudget);
                    }
                }
            }
        }
    }
    reasons
}

/// Choose the session's profile. `core` is always permitted.
pub fn select_profile(inputs: &SelectionInputs) -> ProfileSelection {
    let mut permitted = Vec::new();
    let mut excluded = Vec::new();
    for profile in TrustProfile::ALL {
        let reasons = exclusion_reasons(inputs, profile);
        if reasons.is_empty() {
            permitted.push(profile);
        } else {
            excluded.push(Exclusion { profile, reasons });
        }
    }
    let meets_floor = |p: &&TrustProfile| inputs.floor.is_none_or(|f| p.meets(f));
    let profile = permitted
        .iter()
        .filter(meets_floor)
        .max()
        .or_else(|| permitted.iter().max())
        .copied()
        .unwrap_or(TrustProfile::Core);
    ProfileSelection {
        profile,
        inputs: inputs.clone(),
        excluded,
        not_run: ProfileSignal::ALL
            .into_iter()
            .filter(|s| !profile.includes(*s))
            .collect(),
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
/// evaluation: an attestation that no longer verifies, or a camera whose
/// coverage fell below the profile's minimum. A profile cannot carry a floor
/// on a signal it has lost.
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
