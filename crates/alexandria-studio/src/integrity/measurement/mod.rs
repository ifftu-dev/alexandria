//! Measurement: the labelled session corpus, the offline evaluator that
//! replays it through this crate's scoring code, and the certification
//! manifest that admits a signal to a trust profile with a measured rate.
//!
//! A signal is admitted only by a reviewed, non-synthetic certification;
//! everything else stays advisory (see [`super::decide`]). The evaluator
//! binary is `examples/integrity_evaluate.rs`; its logic is [`cli::run`].

mod certification;
pub mod cli;
mod corpus;
mod report;

pub use certification::{
    Certification, CertifiedProfile, CertifiedSignal, Shortfall, CERTIFICATION_FORMAT,
};
pub use corpus::{
    corpus_digest, digest_bytes, seal, Corpus, CorpusEntry, CorpusManifest, CorpusWindow, EntryRef,
    Provenance, CLEAN_LABEL, CORPUS_FORMAT,
};
pub use report::{evaluate, ProfileReport, Report, SignalMetrics};

use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum MeasurementError {
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{path}: {source}")]
    Parse {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error("{what}: digest {actual} does not match the pinned {expected}")]
    DigestMismatch {
        what: String,
        expected: String,
        actual: String,
    },
    #[error("{0}")]
    Invalid(String),
}
