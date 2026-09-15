# alexandria-verify

Verify [Alexandria](https://github.com/ifftu-dev/alexandria) credentials: W3C
Verifiable Credentials 2.0, `did:key` resolution, JCS canonicalization
(RFC 8785), and detached Ed25519 JWS (RFC 7797, `b64:false`).

`MIT OR Apache-2.0`, while the Alexandria application itself is
AGPL-3.0-or-later. That split is deliberate. Alexandria promises that checking a
credential is free, offline-capable, and permanent — a copyleft verification
library would contradict it, because an HR platform, a registrar, or an ATS
embedding this would have to publish their own product under the AGPL. The
ability to check a signature has to be everywhere to be worth anything.

## No I/O

The crate does not open a socket, a file, or a database. Verification needs
persistent state in four places — the issuer's key at a point in time, a status
list's bits, a local suspension flag, and whether something supersedes the
credential — and each arrives through the `VerificationStore` trait.

```rust
use alexandria_verify::{
    NullStore,
    vc::{AcceptanceDecision, VerificationPolicy, verify::verify_credential},
};

// NullStore supplies no external status or key-registry state. A signed
// credential that references an absent status list remains pending.
let store = NullStore;
let policy = VerificationPolicy::default();
let result = verify_credential(&store, &credential, "2026-08-13T00:00:00Z", &policy);

assert!(result.valid_signature);
if credential.credential_status.is_some() {
    assert_eq!(result.acceptance_decision, AcceptanceDecision::Pending);
}
```

Implement `VerificationStore` over whatever you actually have — SQLite, a
credential bundle, Postgres — and the same verification logic runs against it.
`tests/no_io_deps.rs` fails the build if a dependency that reaches the outside
world is ever added.

## Untrusted input

`json::parse_untrusted` and `json::decode_untrusted` parse bytes from a peer, a
file, or a service under explicit `JsonLimits` before any typed decoding or
signature work. They refuse, as distinct errors:
- documents over the byte limit, checked before parsing;
- nesting deeper than the depth limit;
- arrays, objects, or strings over their limits;
- duplicate object keys at any depth;
- numbers outside JavaScript's exact integer range (±2^53−1) or non-finite;
- trailing bytes.

`vc::decode_credential` applies `vc::CREDENTIAL_JSON_LIMITS`:
- 256 KiB;
- depth 32;
- 4096 array elements;
- 256 object entries;
- 64 KiB strings.

A payload that holds several credentials may have its own outer limits, but
each credential in it must still pass these.
`tests/credential_limits.rs` checks every limit at its exact boundary and one
past it.

## Trust classification

A valid signature says who signed a credential, not that the signer is approved
for anything. `trust::classify_credential` turns a verification result into a
provenance state: `Invalid` (with reason codes), `Pending` (with the missing
evidence), `VerifiedSelfClaim`, `VerifiedIssuerSigned`, or
`VerifiedCourseEndorsement`. It rechecks the supplied result against the
credential, so a result for another credential or an `accept` that contradicts
its own flags is invalid.

A self-claim is only classified as course-endorsed when its signed evidence
references name the exact course document and completion root of a supplied
`CourseCompletionBinding`, the binding matches the expected network and subject,
and distinct authorized attestors meet the signed policy threshold. These states
carry no privilege; policy qualification is a separate decision.

## Interoperability

`tests/vectors/` holds signed credentials with known-good and known-bad
outcomes, plus `independent-verifier.mjs` — a small Node implementation
written against the specification rather than against this code. It passes all
twelve vectors. If you are writing your own verifier in another language, start
there: the vectors are the contract, and this crate is one implementation of it.

The signing input is **raw payload bytes**, not base64url — RFC 7797 with
`b64:false`. This is the detail most independent implementations get wrong.

## Licence

MIT OR Apache-2.0, at your option.
