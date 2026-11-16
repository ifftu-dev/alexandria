//! Coverage statements for this crate's trust profiles and verification
//! rungs.
//!
//! The statement text and every published version of it live in the
//! verifier library's registry ([`alexandria_verify::coverage`]), so any
//! verifier resolves a credential's statement from its digest offline. This
//! module adapts the registry to [`TrustProfile`], [`VerificationRung`] and
//! [`Certification`]: new statements are issued at
//! [`COVERAGE_VERSION`], and a digest of any published version resolves.

use alexandria_verify::coverage::{self as registry, Certified};

use super::measurement::Certification;
use super::{TrustProfile, VerificationRung};

pub use alexandria_verify::coverage::{CertificationRef, CoverageStatement, COVERAGE_FORMAT};

/// The statement text version new sessions are issued against.
pub const COVERAGE_VERSION: &str = registry::CURRENT_VERSION;

/// What a statement embeds from `certification`. A synthetic certification
/// is never embedded, since no session is scored against it.
fn certified(certification: Option<(&Certification, &str)>) -> Option<Certified> {
    let (c, digest) = certification.filter(|(c, _)| !c.synthetic)?;
    Some(Certified {
        reference: CertificationRef {
            version: c.version.clone(),
            date: c.date.clone(),
            digest: digest.to_string(),
            corpus_digest: c.corpus_digest.clone(),
        },
        profiles: c
            .profiles
            .iter()
            .map(|(profile, p)| (profile.clone(), p.signals.clone()))
            .collect(),
    })
}

/// The current-version statement for `profile` and `rung`. `certification`
/// is the certification in force with its digest.
pub fn coverage_statement(
    profile: TrustProfile,
    rung: VerificationRung,
    certification: Option<(&Certification, &str)>,
) -> CoverageStatement {
    registry::statement(
        COVERAGE_VERSION,
        profile.as_str(),
        rung.as_str(),
        certified(certification).as_ref(),
    )
    .expect("the registry has a current statement for every profile and rung")
}

/// Every current-version statement under `certification`, one per profile
/// and rung.
pub fn coverage_statements(
    certification: Option<(&Certification, &str)>,
) -> Vec<CoverageStatement> {
    registry::statements(COVERAGE_VERSION, certified(certification).as_ref())
}

/// The statement of any published version with `digest`, under
/// `certification` or none.
pub fn resolve_coverage(
    digest: &str,
    certification: Option<(&Certification, &str)>,
) -> Option<CoverageStatement> {
    registry::resolve(digest, certified(certification).as_ref())
}
