pub mod aggregation;
pub mod attestation;
pub mod auto_issuance;
pub mod catalog;
pub mod challenge;
pub mod chapters;
pub mod classroom;
pub mod completion;
pub mod courses;
pub mod credentials;
pub mod elements;
pub mod enrollment;
pub mod evidence;
pub mod goal_templates;
pub mod governance;
pub mod graph;
pub mod guardian;
pub mod health;
pub mod identity;
pub mod instructor;
pub mod integrity;
pub mod opinions;
pub mod pinning;
pub mod plugins;
pub mod presentation;
pub mod profile;
pub mod reputation;
pub mod role_assessment;
pub mod sentinel_dao;
pub mod sentinel_gaze;
pub mod sentinel_holdout;
pub mod sentinel_ml;
pub mod sentinel_priors;
pub mod settings;
pub mod snapshot;
pub mod taxonomy;
pub mod username_registry;
pub mod users;

pub mod content;
pub mod p2p;
pub mod pairing;
pub mod ratelimit;
pub mod storage;
pub mod sync;

#[cfg(any(desktop, target_os = "android"))]
pub mod tutoring;

#[cfg(target_os = "ios")]
pub mod tutoring_mobile;
#[cfg(target_os = "ios")]
pub use tutoring_mobile as tutoring;

#[cfg(not(any(desktop, target_os = "ios", target_os = "android")))]
pub mod tutoring_stubs;
