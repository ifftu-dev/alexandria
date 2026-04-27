pub mod catalog;
pub mod device_id;
pub mod discovery;
pub mod gossip;
pub mod governance;
pub mod nat;
pub mod network;
pub mod opinions;
pub mod rate_limit;
pub mod scoring;
pub mod sentinel;
pub mod signing;
pub mod stress;
pub mod sync;
pub mod taxonomy;
pub mod types;
pub mod validation;

// VC / credential / pinning / archive propagation (VC-first migration)
pub mod archive;
pub mod pinboard;
pub mod presentation;
pub mod vc_did;
pub mod vc_fetch;
pub mod vc_status;
