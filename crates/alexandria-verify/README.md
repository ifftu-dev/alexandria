# alexandria-verify

Verify [Alexandria](https://github.com/ifftu-dev/alexandria) credentials: W3C
Verifiable Credentials Data Model 2.0 secured with Data Integrity proofs
(`DataIntegrityProof`, cryptosuite `eddsa-jcs-2022`), Bitstring Status List
revocation, `did:key` resolution, and JCS canonicalization (RFC 8785).

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
twelve credential vectors, the exact-byte limit vectors in
`tests/vectors/limits/`, and the course completion endorsement vectors in
`tests/vectors/endorsements/`. `tests/limit_vectors.rs` and
`tests/endorsement_vectors.rs` check those against this crate, and the app's
endorsement import consumes the same endorsement bytes. If you are writing your own verifier in another language, start there:
the vectors are the contract, and this crate is one implementation of it.

Presentations (`vc::presentation`) are W3C Verifiable Presentations with a
holder `DataIntegrityProof` of purpose `authentication`, bound to the
verifier's `challenge` and `domain` and valid for at most five minutes; the
credential exchange (`exchange`) is built on them.

The proof is the standard `eddsa-jcs-2022` cryptosuite, so a general-purpose
Data Integrity verifier (for example `@digitalbazaar/data-integrity` with
`@digitalbazaar/eddsa-jcs-2022-cryptosuite`) verifies an Alexandria credential
without any Alexandria code. The one detail independent implementations get
wrong is the status list bit order: Bitstring Status List counts from the most
significant bit of each byte.

A credential's `statusListCredential` is either a `urn:` (the list travels in
the issuer's export bundle) or the URL a host serves it at,
`{origin}/status-lists/{issuer}/{n}` — `vc::status::list_url` and
`parse_list_url` go between the two. A verifier that fetches such a URL hands
the document to `vc::status::verify_fetched_list(document, url, issuer,
purpose)`, which accepts it only as that list, issued and signed by that
issuer, and returns the bitstring; the crate does no I/O itself.

## Licence

MIT OR Apache-2.0, at your option.

## Holder-authorized credential exchange

`exchange` defines `alexandria-credential-exchange/1`: an organization request
and a holder-signed disclosure of one complete credential. The signature covers
a domain prefix, a zero byte, and JCS-canonical share bytes. It binds the exact
request, audience, nonce, subject, skill, network, taxonomy digest, and validity
window. Shares expire within five minutes. Receiving applications must load the
expected request from their own store and enforce atomic replay handling.

A new-assessment request additionally requires an `AssessmentCredential` with
`assessment-items-bloom-v1`, terminal integrity, and an evidence reference
`request:<id>:<nonce>`. The receiver must check issuance after invitation and
apply its configured taxonomy policy. This is an authenticity and binding
contract, not proof that the holder-controlled device is tamper-proof.

Only a holder who is also the credential issuer may include `IssuerState` in
the signed disclosure. It represents that issuer's current lifecycle assertion;
it does not turn self-issued evidence into independent corroboration. Third-party
status not supplied through a trusted verification store remains pending.
`verify_share` returns the ordinary accept/pending/reject result. Receivers must
preserve that distinction and record the verification time.
