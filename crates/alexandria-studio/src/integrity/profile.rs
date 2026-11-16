//! Trust profiles: the fixed signal sets a session can run and how the
//! backend chooses one at session start.
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
//! The profile and signal types and the floor judgement ([`evaluate_floor`])
//! live in the verifier library ([`alexandria_verify::integrity`]), so a
//! verifier judges an issuance policy the same way; this module re-exports
//! them and adds selection.

use serde::{Deserialize, Serialize};

pub use alexandria_verify::integrity::{
    evaluate_floor, PolicyFloor, ProfileSignal, TrustProfile, MIN_CAMERA_COVERAGE,
};

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
