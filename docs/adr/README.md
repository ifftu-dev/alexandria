# Architecture decision records

One decision per file, in the [Nygard format](https://cognitect.com/blog/2011/11/15/documenting-architecture-decisions): Status, Context, Decision, Consequences, at most two pages. Files are numbered `NNNN-kebab-title.md`, and numbers are never reused. To reverse a decision, write a new ADR and mark the old one `Status: superseded` with `Superseded-by:`.

Status values:
- `active`: decided, implementation still in progress.
- `implemented`: decided and in the code.
- `superseded`: replaced by a later ADR.

| ADR | Decision | Status | Date |
|---|---|---|---|
| [0001](0001-vc-first-credentials.md) | Verifiable Credentials (VC DM 2.0, `eddsa-jcs-2022`, Bitstring Status List) replace SkillProofs | implemented | 2026-04-24 |
| [0002](0002-bundled-network-profiles.md) | Networks are bundled profiles; each user profile is bound to one `network_id` | implemented | 2026-09-15 |
| [0003](0003-credential-authority.md) | Credential status is issuer-bound; challenge and plugin-attestation authority are retired | implemented | 2026-09-15 |
| [0004](0004-tauri-free-runtime-crate-boundary.md) | The Tauri-free runtime lives behind a crate boundary (`alexandria-studio`) | active | 2026-09-16 |
| [0005](0005-sponsored-batched-anchoring.md) | Credential anchors are batched Merkle roots, paid by an on-chain sponsor instead of a treasury key | active | 2026-11-10 |
| [0006](0006-checkable-integrity-evidence.md) | Integrity evidence is checkable by the relying party or labelled self-reported | implemented | 2026-11-16 |
| [0007](0007-did-key-never-rotates.md) | A `did:key` never rotates; organisations rotate through their domain's key list | implemented | 2026-11-16 |
| [0008](0008-hosted-organisation-issued-credentials.md) | The hosted service issues organisation-signed credentials under the organisation's domain-listed key | implemented | 2026-11-16 |
