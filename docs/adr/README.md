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
