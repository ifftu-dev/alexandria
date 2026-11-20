# Alexandria credential test vectors

Signed credentials with known-correct verification outcomes. Written so that an
independent verifier — in any language — can be checked against this one and
shown to agree.

Nothing here depends on Alexandria being reachable, running, or involved. Each
file is self-contained: the credential, whatever state verification needs, the
verification time, the policy, and the expected result. There is no network
access anywhere in the verification path.

## What a verifier needs

Three primitives, and no more:

- **JCS** — JSON Canonicalization Scheme, [RFC 8785](https://www.rfc-editor.org/rfc/rfc8785)
- **SHA-256**
- **Ed25519** — [RFC 8032](https://www.rfc-editor.org/rfc/rfc8032)

The proof is a W3C Data Integrity proof using the `eddsa-jcs-2022`
cryptosuite ([VC Data Integrity 1.0](https://www.w3.org/TR/vc-data-integrity/),
[VC-DI-EdDSA 1.0 §3.3](https://www.w3.org/TR/vc-di-eddsa/#eddsa-jcs-2022)),
so any conforming Data Integrity verifier accepts it as-is. You do **not** need
JSON-LD tooling: the suite canonicalizes JSON, not RDF. `@context` is covered
by the signature like any other field, and nothing expands or dereferences it.

## The algorithm, in short

1. Take the credential without its `proof`. Canonicalize it with JCS.
2. Take the `proof` without `proofValue`, add the credential's `@context` to
   it, and canonicalize that with JCS. This is the proof configuration.
3. `hashData = SHA-256(canonical proof configuration) || SHA-256(canonical document)` — 64 bytes.
4. `proofValue` is multibase base58btc (`z` prefix) of the 64-byte Ed25519
   signature over `hashData`. Decode it and verify against the issuer's key.
5. Check the proof options: `type` is `DataIntegrityProof`, `cryptosuite` is
   `eddsa-jcs-2022`, `proofPurpose` is `assertionMethod`, and
   `verificationMethod` is `<issuer DID>#<fragment>` — a method the issuer
   controls. For `did:key` the fragment is the key's own multibase identifier.
6. Apply the remaining checks: expiry, subject binding, the revocation and
   suspension status lists, supersession, the coverage statement cross-check,
   answer custody, and the supplied verification policy.
7. Return `accept`, `pending`, or `reject`. Missing issuer-key or referenced
   status-list evidence is pending. A failed cryptographic/policy check or an
   invalid status reference is rejected.

## Presentations

A holder presents credentials as a W3C Verifiable Presentation secured the
same way: `proof.type` `DataIntegrityProof`, cryptosuite `eddsa-jcs-2022`,
`proofPurpose` `authentication`, `challenge` = the verifier's nonce, `domain`
= the verifier's audience, `created` and `expires` at most 300 seconds apart,
`verificationMethod` = `<holder DID>#<fragment>`. The hash data is computed
exactly as for a credential (steps 1–4 above) over the presentation document.
Verify the holder's signature first, then each credential inside
`verifiableCredential` on its own terms.

## Resolving the issuer key

`did:key` is self-resolving: the public key is embedded in the identifier, so no
lookup is required in the common case. Decode the multibase (`z`, base58btc),
strip the `0xed 0x01` multicodec prefix, and the remaining 32 bytes are the
Ed25519 public key.

A `did:key` always resolves to the key in its identifier. `store.keyRegistry`
is consulted only for a DID method that is not self-resolving, and an entry
for a `did:key` is ignored: `08-did-key-ignores-registry.json` supplies one
naming another key and still accepts.

The issuer is either the DID string or an object whose `id` is the DID, with
other properties (an organisation's `name` and `domain`) covered by the proof
like any other field. Resolve the key and check the verification method
against `id`.

## What a bare credential cannot tell you

`did:key` is self-resolving, so the signature verifies with nothing but the
document in front of you. Status does not: a credential names its revocation
and suspension lists; it does not carry them. That needs no contact with
Alexandria, only having been given the lists, which is what a credential
*bundle* is for: it carries the status lists next to the credentials, and
verifies entirely offline. A status
list named by an `https` URL (`{origin}/status-lists/{issuer}/{n}`) can also be
fetched from that URL: the document served there is the signed
`BitstringStatusListCredential` whose `id` is the URL. A verifier that fetches
one checks `id`, issuer and signature before reading a bit — the reference
implementation is `verify_fetched_list` in `src/vc/status.rs`, and
`scripts/demo/verify-credential.mjs` in the app shows the same check in Node.
The vectors themselves stay offline: their lists are supplied in `store`.

## Status lists

`credentialStatus` is one [Bitstring Status List](https://www.w3.org/TR/vc-bitstring-status-list/)
entry (`BitstringStatusListEntry`) or a list of them. Alexandria issues two at
the same index: `statusPurpose` `revocation` (list number 1) and `suspension`
(list number 2). Each entry's bit decides only its own purpose; an entry of
another purpose is skipped. `statusListIndex` is a bit index into the
list named by `statusListCredential`. Bit *n* is byte `n / 8`, bit `n % 8`
counted **from the most significant bit** — so index 9 is byte 1, mask `0x40`.
Getting this backwards is the single most common interoperability bug, which
is why `07-revoked.json` exists. The published list is a
`BitstringStatusListCredential` whose subject carries the bitstring GZIP-compressed
and multibase base64url encoded (`u…`); the vectors supply the decoded bytes as hex.

A status list the verifier does not have is not evidence of revocation or active
status. The verifier returns `pending` with `status_list_missing` (or
`suspension_list_missing` for the suspension entry); it does not accept the
credential or label it revoked. A storage failure is separately reported as
`status_list_unavailable` (`suspension_list_unavailable`). A malformed or out-of-range bit index
sets `statusValid` to false and rejects the credential.

## Coverage statements

An integrity assertion names the coverage statement it was issued under by
`coverageStatementDigest` (`blake3:` + 64 hex). A credential may name it again
in `relatedResource` as `{ id, digestSRI, mediaType }`, where `id` is
`urn:alexandria:coverage-statement:{digest}` or a URL ending in
`/coverage-statements/{digest}`. When the digest in any such `id` differs from
`coverageStatementDigest`, the credential names two statements: set
`coverageStatementMismatch` and reject. The check needs no registry and no
BLAKE3. `digestSRI` is for readers who fetch the statement bytes; the
statement texts themselves are in the crate's `coverage` registry.

## Answer custody

An integrity assertion may state `answerCustody`: `device` when the candidate's
device held the items and their answers, `issuer` when the answers never left
the issuer. An assertion without it reads as `device`. A credential is
self-issued when its issuer DID is its subject's.

An organisation's role credential from a hosted attempt carries one `evidence`
entry of type `HostedAttemptEvidence`: `attemptId`, `answerCustody` (`issuer`),
`gradedBy` (`issuer`), the issuer's `score` (a fraction in [0,1], omitted when
it states none), the issuer's `probe` summary (omitted when no probe step ran),
the `attestation` outcome and the `verifiedAssuranceLevel` the issuer found.

Set `custodyDefects` (omitted from `expect` while empty), in this order, and
reject when any is set:

| Code | When |
|---|---|
| `issuer_custody_self_issued` | A self-issued credential states `issuer` custody: nobody holds answers away from themselves |
| `issuer_custody_unproven` | A credential that is not self-issued states `issuer` custody with no `HostedAttemptEvidence` entry: only a hosted attempt shows the issuer held the answers, and a candidate's second key is not self-issued by the DID test |
| `hosted_attempt_self_issued` | A self-issued credential carries a `HostedAttemptEvidence` entry |
| `hosted_attempt_invalid` | More than one entry, or one that does not parse, does not say `answerCustody` and `gradedBy` `issuer`, or states a `score` outside [0,1] |
| `hosted_attempt_custody_mismatch` | An entry beside an assertion that does not state `issuer` custody, or beside no assertion |

## Organisation backing

A vector may supply `organisationDescriptor`, the `alexandria-org/1` record an
organisation publishes in its domain's DNS TXT record (`domain`, `name`,
`keys`: one to eight `did:key` identifiers), and `expectOrganisationBacking`.
Backing is judged apart from the verification result, which it never changes:

| Outcome | When |
|---|---|
| `descriptor_not_supplied` | No descriptor was given |
| `invalid_descriptor` | The descriptor fails its own checks (format, normalised domain, name, key list) |
| `wrong_domain` | The issuer object names a different `domain` |
| `key_not_listed` | The descriptor does not list the issuer key. The signature still verifies |
| `verified` | Listed: report the descriptor's `name` and `domain`, not the issuer object's |

## File format

```jsonc
{
  "description": "what this case asserts, in a sentence",
  "verificationTime": "2026-06-01T00:00:00Z",   // the `now` to verify against
  "policy": { ... },                            // acceptance rules
  "credential": { ... },                        // the signed credential
  "store": {                                    // optional; empty = no local context
    "keyRegistry":  [ { "did", "keyId", "publicKeyHex" } ],  // non-did:key issuers only
    "statusLists":  { "<list id>": "<hex bytes>" },  // revocation and suspension lists
    "superseded":   [ "<credential id>" ]
  },
  "organisationDescriptor": { ... },            // optional, see above
  "expectOrganisationBacking": { "outcome": ... },  // optional
  "expect": { ... }                             // the required result
}
```

Compare at least `validSignature`, `issuerResolved`, `statusValid`, `expired`,
`revoked`, `suspended`, `subjectBound`, `coverageStatementMismatch` (omitted
from `expect` while false), `custodyDefects` (omitted while empty),
`pendingReasons`, and `acceptanceDecision`. Every
`expect` here also carries `attestation` (`not_supplied`) and
`verifiedAssuranceLevel` (`local`); the attestation set below exercises them. The
booleans matter independently of the decision: policy changes the decision,
never the facts. `09` and `05` are the same expired credential, and differ only
in what the policy does about it.

## The suite

| Vector | Asserts |
|---|---|
| `01-valid` | The happy path: signature, self-resolution, acceptance |
| `02-tampered-payload` | A claim altered after signing. The sharpest test of a JCS implementation — a verifier that canonicalizes differently may wrongly accept |
| `03-wrong-signing-key` | Signed by a key the issuer DID does not name |
| `04-malformed-proof-value` | `proof.proofValue` is not a multibase Ed25519 signature. Must reject, not error |
| `05-expired` | `validUntil` precedes the verification time |
| `06-non-did-subject` | Subject id is not a DID, so nothing is bound |
| `07-revoked` | Status-list bit set. Pins the bit-order convention |
| `08-did-key-ignores-registry` | Registry state names another key for a `did:key` issuer; it is ignored and the credential accepted |
| `09-expired-permissive-policy` | Same expired credential, policy does not reject. `expired` stays true |
| `10-type-not-allowed` | Every cryptographic check passes; policy still rejects |
| `11-missing-status-list` | Signature verifies, but the referenced status list is absent; decision is pending |
| `12-missing-issuer-key` | A non-self-resolving issuer has no supplied key binding; decision is pending |
| `13-coverage-statement-v1` | A version-1 statement named by digest alone; accepted |
| `14-coverage-statement-v2` | A version-2 statement named by digest and by a `relatedResource` URL; accepted |
| `15-coverage-statement-mismatch` | `relatedResource` names a different statement; rejected with `coverageStatementMismatch` |
| `16-suspended` | Revocation and suspension entries; the suspension bit is set, so rejected and not revoked |
| `17-reinstated` | The same credential with the suspension bit cleared; accepted |
| `18-suspension-list-missing` | Suspension list absent; pending with `suspension_list_missing` |
| `19-object-issuer` | Issuer object with `name` and `domain`; accepted, backing `descriptor_not_supplied` |
| `20-organisation-backing-verified` | The descriptor lists the key; backing `verified` with the descriptor's name |
| `21-organisation-key-not-listed` | The key was removed from the descriptor; backing `key_not_listed`, still accepted |
| `22-organisation-wrong-domain` | A descriptor for another domain; backing `wrong_domain` |
| `23-organisation-invalid-descriptor` | A descriptor that fails its checks; backing `invalid_descriptor` |
| `24-device-custody-self-issued` | A self-issued credential stating `device` custody under a version-3 statement; accepted |
| `25-issuer-custody-self-issued` | A self-issued credential stating `issuer` custody; rejected with `issuer_custody_self_issued` |
| `26-hosted-attempt-issuer-signed` | An organisation's role credential with a `HostedAttemptEvidence` entry and `issuer` custody; accepted |
| `27-hosted-attempt-self-issued` | A hosted attempt entry on a self-issued credential; rejected with `issuer_custody_self_issued` and `hosted_attempt_self_issued` |
| `28-hosted-attempt-custody-mismatch` | A hosted attempt entry beside `device` custody; rejected with `hosted_attempt_custody_mismatch` |
| `29-hosted-attempt-graded-by-device` | A hosted attempt entry that says the device graded; rejected with `hosted_attempt_invalid` |
| `30-issuer-custody-unproven` | A credential signed by a key other than its subject's stating `issuer` custody with no hosted attempt entry; rejected with `issuer_custody_unproven` |
| `31-hosted-attempt-score-out-of-range` | A hosted attempt entry stating a score above 1; rejected with `hosted_attempt_invalid` |

The suite contains accepted, rejected, and pending cases, including independent
signature, expiry, subject-binding, revocation, suspension, type-policy,
coverage-statement, answer-custody, organisation-backing and missing-evidence
outcomes.

## Attestation vectors

`attestation/` holds credentials with and without a `DeviceAttestationEvidence`
entry in `evidence`. Each file adds `trust`, the inputs a verifier is given:
`apple` (`rootPem`, `appId`, `allowDevelopment`) and `playIntegrity`
(`packageName`, response `keys`). The roots and keys are synthetic stand-ins for
Apple's and Google's. `expect` holds `attestation` (`outcome`, plus `platform`
or the invalid `reason`), `verifiedAssuranceLevel` and `acceptanceDecision`.

The cases cover not supplied, credential-bound on iOS, Android and a companion
phone, session-bound from the device and from a companion phone, unverifiable without Play Integrity keys, and invalid
evidence (untrusted chain, wrong nonce, wrong app id, evidence lifted from
another credential, a companion binding for another learner, duplicate
entries). Only verified credential-bound evidence from the attempt's own device
reads `device_attested`; a companion phone's reads `companion_attested`, and
either binding from a companion says `companion: true`; invalid evidence
rejects; every other outcome leaves the decision to the rest
of verification. The binding digest, the per-platform proofs and the outcome
rules are documented in the crate's `attestation` module and in
`tests/attestation_vectors.rs`. The independent verifier below does not check
this set.

## Issuance policy vectors

`issuance-policy/` holds the shared issuance rule that an issuing app and a
hosted service both apply. Each file is a `policy`, the `integrity` assertion,
the `attestation` outcome a verifier established and, for a role credential,
the `assessed` entry naming the credential it was issued on or the `hosted`
entry naming the hosted attempt it was issued from. `expect.refusal` is `null`
or the first unmet requirement with its stable `code`, in the order custody
(`custody_below_required`), status, score, flags, assurance, floor, rung,
probes. A required answer custody of `issuer` is met only by a role
credential from a hosted attempt: an assertion stating `issuer` beside a
`hosted` entry that says the issuer held the answers and graded. Without such
an entry the rule credits `device` whatever the assertion states, since anyone
can sign an assertion stating `issuer`; `device` accepts either (vectors
32–36, 52–53). A required
assurance level is met only by the verified level, and a companion phone's
evidence verifies `companion_attested`. A floor's device attestation signal
counts only for verified evidence from the attempt's own device, never a
companion's (47–48). A degraded signal never carries a floor, except the
camera when the assertion states a `cameraCoverage` from 0.9 to 1 (23–26). The
required rung (`min_verification_rung`) is judged on the strict ladder: a
stated `probed` or higher counts only beside a probe step with outcome
`satisfied`, at least one satisfied probe and none failed or skipped, whatever
its policies (an authored variant served under `reseed` earns `probed`);
otherwise the rule credits `process`, and the refusal names the credited rung
(37–46). A hosted entry supplies the verified level, the floor's attestation
and the probe step judged, in place of the assertion's own (49–51).
`tests/policy_vectors.rs` documents the format.


## Untrusted input limits

Before typed decoding or any signature work, a credential document is parsed
as strict JSON under these limits:

| Limit | Value |
|---|---|
| Document bytes (checked before parsing) | 262,144 |
| Nesting depth (the credential object is 1) | 32 |
| Elements in one array | 4,096 |
| Entries in one object | 256 |
| UTF-8 bytes in one decoded string or key | 65,536 |

A verifier refuses a document as the first of these that applies:

| Outcome | Refused when |
|---|---|
| `too_large` | The document exceeds the byte limit |
| `too_deep` | An array or object would open beyond the depth limit. This is checked on entering it, even when empty |
| `too_many_elements` | An array receives one element more than the limit |
| `too_many_entries` | An object receives one entry more than the limit |
| `string_too_long` | A decoded string or key exceeds the string limit |
| `duplicate_key` | An object repeats a key at any depth |
| `unsafe_number` | An integer literal is outside ±(2^53−1), or a fractional/exponent literal's magnitude exceeds 2^53−1 |
| `invalid` | Malformed JSON, including invalid UTF-8, a byte-order mark, a lone surrogate escape, a literal that overflows a double, or trailing bytes |

Checks happen in document order. An array element is parsed before the count is
checked. An object key is length-checked, then duplicate-checked, then counted,
and only then is its value parsed.

`limits/manifest.json` lists the exact byte files in `limits/` and the outcome
each must produce. Every case starts from the credential in `01-valid.json` and
changes one structural property, so a failure names exactly one limit. `accept`
means the bytes pass these limits; it says nothing about the signature.

## Course completion endorsements

An instructor endorses a learner's course completion by signing its completion
binding. `endorsements/manifest.json` supplies:
- the signing `domain`;
- the endorsement's structural `limits`;
- the signed course `policy`;
- the `expectedBinding` for the completion;
- the outcome of each byte file in `endorsements/`;
- `thresholds` evaluated over sets of those files.

To verify one endorsement against the policy and expected binding:

1. Parse the exact bytes as strict JSON under the manifest limits (see the
   previous section). A structural refusal is the outcome.
2. Its `binding` must equal the expected binding, compared as JCS. If not:
   `binding_mismatch`.
3. `attestor_did` must name an entry in `policy.authorized_attestors`, and
   `attestor_public_key_hex` must equal that entry's key exactly. If not:
   `unauthorized_attestor`.
4. The key must be 64 lowercase hex digits (else `invalid_public_key`). Its
   `did:key` must equal `attestor_did` (else `identity_mismatch`). A
   `did:key` is `z` plus base58btc of `0xed 0x01` plus the 32 key bytes.
5. `signature_hex` must be 128 lowercase hex digits. It must be a valid
   Ed25519 signature over `domain || 0x00 || JCS(expected binding)`. If
   either check fails: `invalid_signature`.
6. Otherwise the outcome is `valid`.

For a threshold, count each attestor DID at most once, and only for a `valid`
endorsement. Every other endorsement in the set is rejected. The policy is
satisfied when the count reaches `required_attestors`.

## A worked implementation

`independent-verifier.mjs` in this directory is a complete verifier written from
this document alone — no Alexandria library, nothing but Node's standard crypto,
JCS implemented by hand. It passes every credential vector in this directory
(including organisation backing), the limit vectors and the endorsement
vectors, and runs from any directory.

```sh
node crates/alexandria-verify/tests/vectors/independent-verifier.mjs
```

It is here as evidence, not as shipped code. If it stops passing, either this
document has drifted from the format or the format changed without the document
being updated — and both are bugs in the promise that anyone can verify.

## Regenerating

```sh
ALEXANDRIA_REGENERATE_VECTORS=1 cargo test -p alexandria-verify --test vectors
ALEXANDRIA_REGENERATE_VECTORS=1 cargo test -p alexandria-verify --test limit_vectors
ALEXANDRIA_REGENERATE_VECTORS=1 cargo test -p alexandria-verify --test endorsement_vectors
ALEXANDRIA_REGENERATE_VECTORS=1 cargo test -p alexandria-verify --test policy_vectors
ALEXANDRIA_REGENERATE_VECTORS=1 cargo test -p alexandria-verify --features attestation --test attestation_vectors
```

Signing keys derive from fixed byte patterns, so regeneration is deterministic
and produces byte-identical files. If a regeneration changes a file, the wire
format moved — which is worth seeing in a diff rather than discovering in
somebody else's implementation.

The keys here are published on purpose and secure nothing.
