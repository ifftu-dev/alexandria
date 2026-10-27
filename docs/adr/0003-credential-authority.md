# 0003. Credential status is issuer-bound; challenge and plugin-attestation authority are retired

Status: implemented
Date: 2026-09-15

## Context

After the VC cutover ([0001](0001-vc-first-credentials.md)), two experiments gave parties other than the issuer power over credentials:

- **Credential challenges and escrow.** An off-chain committee could vote on a challenge to a credential, backed by a Cardano escrow validator (`challenge_escrow.ak`), and the outcome could change the issuer's status list. There was no complete authority policy for who sat on the committee or why its vote should bind an issuer.
- **Plugin attestations.** Signed attestations gossiped on `/alexandria/plugin-attestations/1.0` were persisted and treated as trust in a plugin's grader. Any grader with the right attestations could then issue `AssessmentCredential`s.

Both let an under-specified authority mutate or mint credential state.

## Decision

1. **Status changes are issuer-bound.** Only a credential's issuer can revoke, suspend or reinstate it. The command implementations check the issuer DID against both the credential and its status list (`commands/credentials.rs`).
2. **The challenge and escrow experiment is deleted**: the `commands::challenge` module, challenge domain types and UI, escrow transaction builders, the recovery worker, the Aiken challenge validator, and the challenge tables (absent from the baseline schema).
3. **Plugin attestations grant no authority.** Ingest and status IPC and inbound persistence are deleted, along with the attestation and advisory tables. The topic is still subscribed for wire compatibility and is gated as privileged, but no handler persists anything from it.
4. **Plugin credentials need exact bundled bytes.** Grading with any plugin is allowed, because the sandbox is deterministic and practice is harmless. Issuing a credential from a grade is allowed only if the installed manifest bytes and the executed grader bytes exactly match a bundle embedded in the signed app (`credential_trust` in `commands/plugins.rs`). This stays until a policy-backed community grader path is implemented.

## Consequences

- No committee or peer can change another issuer's credential. A disputed credential is a matter for the issuer, or for whoever relies on it.
- Third-party plugins can be used for practice but can't produce credentials until the community grader policy exists.
- Retired command names are listed in `scripts/retired-tauri-commands.json`, and the command check rejects them if they come back.
- Removing the reserved topic needs a coordinated app, relay and monitoring change ([0002](0002-bundled-network-profiles.md)).
