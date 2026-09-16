//! Exact course-completion endorsement views.
//!
//! The signed artifact and policy types live in `alexandria-verify` so every
//! consumer uses the same canonical bytes and verifier. This module only owns
//! the application status DTO.

use serde::{Deserialize, Serialize};

pub use alexandria_verify::course::{
    CourseCompletionBinding, CourseCompletionEndorsement, CourseCompletionPolicy,
};
use alexandria_verify::Did;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CourseCompletionEndorsementStatus {
    pub claim_id: String,
    pub required_attestors: u16,
    pub valid_attestors: Vec<Did>,
    pub rejected_endorsements: usize,
    pub satisfied: bool,
    pub endorsements: Vec<CourseCompletionEndorsement>,
}
