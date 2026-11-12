//! Session fusion: per-signal log-likelihood ratios accumulated per channel,
//! compared with a per-profile threshold, and the decision rule.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::flags::{channel, is_tamper_flag, severity, Severity};

/// Version of the manifest that carries no measured ratios.
pub const UNCERTIFIED_MANIFEST_VERSION: &str = "uncertified";

/// The profile a session is scored under until profile selection exists.
pub const DEFAULT_PROFILE: &str = "core";

/// Measured likelihood ratios per trust profile. The certification step
/// writes real ones from the measurement corpus; until then the manifest is
/// [`RatioManifest::uncertified`] and every signal is advisory.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RatioManifest {
    pub version: String,
    #[serde(default)]
    pub profiles: BTreeMap<String, ProfileRatios>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileRatios {
    /// Accumulated log-likelihood ratio at which a channel counts as over
    /// threshold.
    pub threshold: f64,
    /// Log-likelihood ratio a signal adds each window it is observed.
    #[serde(default)]
    pub ratios: BTreeMap<String, f64>,
}

impl RatioManifest {
    pub fn uncertified() -> Self {
        Self {
            version: UNCERTIFIED_MANIFEST_VERSION.into(),
            profiles: BTreeMap::new(),
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        for (name, profile) in &self.profiles {
            if !profile.threshold.is_finite() || profile.threshold <= 0.0 {
                return Err(format!(
                    "profile {name}: threshold must be finite and positive"
                ));
            }
            if profile.ratios.values().any(|r| !r.is_finite()) {
                return Err(format!("profile {name}: ratios must be finite"));
            }
        }
        Ok(())
    }

    fn profile(&self, profile: &str) -> Option<&ProfileRatios> {
        self.profiles.get(profile)
    }
}

/// What a session's committed chain says, in order.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SessionEvidence {
    /// Flags of each committed snapshot.
    pub windows: Vec<Vec<String>>,
    /// A verification step (probe, consistency) failed.
    pub verification_failed: bool,
    /// Interview checks are never suspended; their worst outcome is flagged.
    pub interview: bool,
}

/// Ordered weakest to strongest; a live session only ever moves up.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Active,
    Completed,
    Review,
    Flagged,
    Suspended,
}

impl SessionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            SessionStatus::Active => "active",
            SessionStatus::Completed => "completed",
            SessionStatus::Review => "review",
            SessionStatus::Flagged => "flagged",
            SessionStatus::Suspended => "suspended",
        }
    }

    pub fn parse(status: &str) -> Option<Self> {
        Some(match status {
            "active" => SessionStatus::Active,
            "completed" => SessionStatus::Completed,
            "review" => SessionStatus::Review,
            "flagged" => SessionStatus::Flagged,
            "suspended" => SessionStatus::Suspended,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChannelEvidence {
    pub channel: String,
    /// Accumulated log-likelihood ratio; zero when no signal has a ratio.
    pub llr: f64,
    pub over_threshold: bool,
    pub signals: Vec<String>,
}

/// What the fusion saw, for the snapshot response and later the assertion.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FusionSummary {
    pub manifest_version: String,
    pub profile: String,
    /// `None` when the manifest certifies nothing for this profile.
    pub threshold: Option<f64>,
    pub channels: Vec<ChannelEvidence>,
    /// Observed signals with no measured ratio: they contributed zero.
    pub advisory: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Decision {
    pub status: SessionStatus,
    pub summary: FusionSummary,
}

/// Applies the decision rule to a session's evidence.
///
/// - A tamper flag suspends (an interview is flagged instead).
/// - A failed verification, or two independent channels over threshold, is
///   flagged: the only credential-affecting outcome short of tamper.
/// - Any warning or critical flag, or one channel over threshold, is review.
/// - Otherwise the session is active, or completed once it ends.
pub fn decide(
    evidence: &SessionEvidence,
    manifest: &RatioManifest,
    profile: &str,
    ending: bool,
) -> Decision {
    let ratios = manifest.profile(profile);
    let threshold = ratios.map(|p| p.threshold);
    let mut channels: BTreeMap<&str, (f64, Vec<String>)> = BTreeMap::new();
    let mut advisory: Vec<String> = Vec::new();
    let mut tamper = false;
    let mut reviewable = false;
    for flag in evidence.windows.iter().flatten() {
        if is_tamper_flag(flag) {
            tamper = true;
            continue;
        }
        reviewable |= severity(flag) != Severity::Info;
        let entry = channels.entry(channel(flag)).or_default();
        if !entry.1.contains(flag) {
            entry.1.push(flag.clone());
        }
        match ratios.and_then(|p| p.ratios.get(flag.as_str())) {
            Some(llr) => entry.0 += llr,
            None if !advisory.contains(flag) => advisory.push(flag.clone()),
            None => {}
        }
    }
    advisory.sort();
    let channels: Vec<ChannelEvidence> = channels
        .into_iter()
        .map(|(name, (llr, signals))| ChannelEvidence {
            channel: name.to_string(),
            llr,
            over_threshold: threshold.is_some_and(|t| llr >= t),
            signals,
        })
        .collect();
    let over = channels.iter().filter(|c| c.over_threshold).count();
    let status = if tamper {
        if evidence.interview {
            SessionStatus::Flagged
        } else {
            SessionStatus::Suspended
        }
    } else if evidence.verification_failed || over >= 2 {
        SessionStatus::Flagged
    } else if reviewable || over == 1 {
        SessionStatus::Review
    } else if ending {
        SessionStatus::Completed
    } else {
        SessionStatus::Active
    };
    Decision {
        status,
        summary: FusionSummary {
            manifest_version: manifest.version.clone(),
            profile: profile.to_string(),
            threshold,
            channels,
            advisory,
        },
    }
}
