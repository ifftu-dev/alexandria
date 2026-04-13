//! Per-evidence weight components (§14.3–§14.8). Stubs — PR 6.

use crate::crypto::did::Did;
use crate::domain::vc::CredentialType;

use super::{AggregationConfig, AggregationInput};

pub fn issuer_weight(_issuer: &Did, _config: &AggregationConfig) -> f64 {
    unimplemented!("PR 6")
}

pub fn type_weight(_credential_type: CredentialType, _config: &AggregationConfig) -> f64 {
    unimplemented!("PR 6")
}

pub fn freshness_weight(
    _evidence: &AggregationInput,
    _skill_id: &str,
    _verification_time: &str,
    _config: &AggregationConfig,
) -> f64 {
    unimplemented!("PR 6")
}

pub fn quality_weight(_evidence: &AggregationInput, _config: &AggregationConfig) -> f64 {
    unimplemented!("PR 6")
}

pub fn independence_weight(
    _evidence: &AggregationInput,
    _peers: &[AggregationInput],
    _config: &AggregationConfig,
) -> f64 {
    unimplemented!("PR 6")
}
