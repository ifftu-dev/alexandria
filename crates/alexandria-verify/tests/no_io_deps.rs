//! The point of this crate is that verifying a credential does not require an
//! application. That property is easy to state and easy to lose: one `use` of
//! a database handle, one convenience dependency, and a server or a third-party
//! verifier is back to linking SQLite, two P2P stacks and a Cardano client to
//! check an Ed25519 signature.
//!
//! So it is asserted here rather than left to review.

use std::collections::BTreeSet;

/// Crates that would drag I/O, a platform, or an application in with them.
///
/// This is a denylist rather than an allowlist because the transitive tree
/// legitimately contains dozens of small pure crates (`subtle`, `zeroize`,
/// `itoa`…) and enumerating them would turn every routine version bump into a
/// test failure. What matters is that none of *these* appear.
const FORBIDDEN: &[&str] = &[
    // persistence
    "rusqlite",
    "libsqlite3-sys",
    "sqlx",
    "diesel",
    // the application and its shell
    "tauri",
    "alexandria-node",
    // networking / P2P
    "libp2p",
    "iroh",
    "iroh-blobs",
    "iroh-gossip",
    "quinn",
    "hyper",
    "reqwest",
    "tokio",
    // chain
    "pallas",
    "pallas-primitives",
    // filesystem / OS access
    "dirs",
    "sysinfo",
    "notify",
];

#[test]
fn crate_links_no_io_or_platform_dependencies() {
    let out = std::process::Command::new(env!("CARGO"))
        .args([
            "tree",
            "-p",
            "alexandria-verify",
            "--edges",
            "normal",
            "--prefix",
            "none",
            "--no-dedupe",
        ])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("run cargo tree");

    assert!(
        out.status.success(),
        "cargo tree failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let tree = String::from_utf8_lossy(&out.stdout);
    let names: BTreeSet<&str> = tree
        .lines()
        .filter_map(|l| l.split_whitespace().next())
        .collect();

    let found: Vec<&&str> = FORBIDDEN.iter().filter(|f| names.contains(**f)).collect();

    assert!(
        found.is_empty(),
        "alexandria-verify must stay I/O-free, but its dependency tree now \
         contains {found:?}.\n\n\
         If this is deliberate, the right move is almost certainly to put the \
         new code behind `VerificationStore` in the consuming crate instead — \
         see the module docs in src/lib.rs."
    );
}

/// `--edges normal` above excludes dev-dependencies, which is intentional: a
/// test-only dependency does not end up in a downstream verifier's binary.
/// This test pins the reason so nobody "fixes" the flag later.
#[test]
fn dev_dependencies_are_deliberately_not_checked() {
    // Nothing to assert — the comment above is the point. Kept as a test so it
    // shows up next to the check it explains.
}
