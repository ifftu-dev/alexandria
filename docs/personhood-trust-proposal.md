# Personhood issuer trust and account binding — proposal

Status (2026-09-30): account-bound private synthetic receipts are implemented and
device-tested. The release-bundled real-document policy parser and issuer checks
are implemented, but no production policy or issuer is activated and real input
remains disabled. The selected first real-input outcome is a private receipt
with no account permissions change. Reviewed app releases will carry policy
updates. The agreed pilot limits are seven-day document freshness, five-minute
future clock skew, and at most 24-hour receipts. Account-creation, voting,
reputation, and other privileges remain out of scope.

## What the receipt would mean

The proposed claim is narrow: at verification time, the submitting Alexandria
account controlled its signing key and presented a valid proof of a document
signature accepted by a named policy. It is not a claim of liveness, physical
possession by the document's subject, or one-person-one-account.

The pinned circuit exposes nine public signals: issuer-key hash, nullifier,
document timestamp, four optional attributes, nullifier seed, and signal hash.
The circuit binds the signal hash and computes the nullifier from the seed and
photo data. See the [pinned verifier circuit](https://github.com/anon-aadhaar/anon-aadhaar/blob/4dad918761cfb1d7d5ed9918dcd796d0cb23ae82/packages/circuits/src/aadhaar-qr-verifier.circom)
and [nullifier helper](https://github.com/anon-aadhaar/anon-aadhaar/blob/4dad918761cfb1d7d5ed9918dcd796d0cb23ae82/packages/circuits/src/helpers/nullifier.circom).

Consequently, changing a document photo can change this identifier, and shared
photo bytes can collide at the identity-policy level without breaking the hash.
The existing circuit is insufficient grounds for promising durable human
uniqueness. Proof validity also does not prevent someone sharing signed document
data with another account.

## Trust configuration

`alexandria_verify::personhood::PinnedDocumentPolicy` loads a versioned policy
whose exact JCS bytes must match `personhood_policy_digest` in the network
profile. Preprod profile revision 2 sets this optional field to `null` and
embeds no document: real-document policy is disabled. A missing field likewise
means disabled for older profiles. A partial configuration, digest mismatch,
wrong network, or malformed policy fails application setup. Policy expiry is
checked whenever public signals are evaluated, including eventual acceptance.
The checker is a foundation API; real-input challenge/receipt integration is
still outstanding.

The implemented schema and remaining receipt authority boundary are:

| Field | Rule |
| --- | --- |
| `schema_version`, `network_id`, `policy_version` | Schema 1, active network, nonzero policy version. The digest is computed over the document and pinned separately in the network profile. |
| `kind`, `document_format` | Exactly `local_document_receipt` and `aadhaar_secure_qr_v2`. |
| `circuit_id`, `verification_key_sha256` | Select a bundled verifier and exact key. Reject caller-supplied keys, URLs, and circuit substitutions. |
| `issuers` | Unique IDs, circuit-key hashes and certificate SHA-256 digests, official HTTPS source, validity intervals, and active/disabled/revoked status. The bundled synthetic issuer is rejected. Certificate provenance still requires release review. |
| `valid_from`, `valid_until` | Required policy interval, start inclusive and end exclusive. Stale policy cannot accept a proof. |
| `nullifier_scope`, `nullifier_seed` | A verifier-selected, fixed network-and-purpose scope; all accepting verifiers must require the exact seed. Do not let clients choose it. |
| `max_document_age_seconds` | Pilot limit 604800 (seven days); the parser permits only positive values up to this ceiling. At the exact freshness expiry the document is rejected. |
| `max_future_skew_seconds` | Pilot limit 300; larger allowances are rejected. |
| `challenge_ttl_seconds` | Positive value at most 120, measured by the issuing verifier. |
| `receipt_ttl_seconds` | Positive value at most 86400; actual expiry is the earliest of this deadline, document freshness expiry, issuer expiry, and policy expiry. |

All four attribute outputs must be zero; this is enforced directly rather than
configured by a policy field. The policy is capped at 32 KiB and 32 issuers;
unknown fields, duplicate JSON keys, noncanonical bytes and scalar aliases fail.
The issuer must be active and valid both now and at the document timestamp.
No allowance extends issuer or policy validity. The public-signal check does
not itself verify Groth16 proofs, account signatures, challenges, or replay state.

Existing relay receipt keys are not implicitly authorized for personhood.
Production issuer material must be obtained through a reviewed authoritative
channel and its circuit key hash independently reproduced. Unknown keys,
unavailable required status, expired policies, and revoked issuers cannot produce
an accepted receipt. A
document timestamp alone is not evidence that a compromised key signed before
compromise; a revoked issuer must not be grandfathered solely on that timestamp.

Updates arrive only in reviewed app releases, with an explicit policy expiry.
There is no remote fetch or fallback to synthetic trust. Revocation changes
therefore reach existing installations only when they update; policy expiry
bounds stale acceptance but cannot provide immediate revocation. A release must
review certificate purpose, chain/status, DER fingerprint, validity, circuit key
hash, policy lifetime and seed continuity before embedding canonical bytes and
their digest together. No production expiry date or seed has yet been selected.

The existing synthetic verifier is local, uses only a test policy, and issues a
clearly marked diagnostic receipt. Local clock and database state are not an
authority against a device owner. A remotely relied-on receipt
requires a separately designated verifier with authoritative time and storage.
That authority is not declared by the document-issuer policy. Document issuers
and Alexandria receipt signers are separate roles.

### Production certificate review still required

The [UIDAI certificate catalogue](https://uidai.gov.in/en/data-and-download),
checked on 2026-09-30, lists a 2026 certificate under Paperless Offline eKYC,
while its Secure QR subsection lists certificates expiring in 2021 and 2020.
The [2026 certificate](https://backend.uidai.gov.in/get/files/media/document/2026-07/uidai_offline_publickey_2026.cer)
was downloaded and inspected with OpenSSL: RSA-2048, exponent 65537, validity
2026-02-03 09:56:12 UTC through 2029-02-03 17:28:36 UTC, DER SHA-256 fingerprint
`e0304b9e61ee3640ecddae2db4b617f2e2678f57dbc2826c2f86ac5c04f277df`.
Inspection is not chain/status validation or evidence that this key signs the
QR format accepted by the pinned circuit. Its production circuit hash has not
been independently reproduced. No downloaded certificate is trusted or bundled
by this change. Confirm those properties before enabling real input.

## Challenge and account binding

The account is its network-bound Alexandria identity, resolved through the
existing DID/key-registry rules. A local profile UUID is not an account identity.
No account key is derived from document contents.

1. The unlocked account requests a challenge for `personhood-receipt-v1` and
   proves control of its current signing key. The verifier checks the requested
   network and purpose before issuing it.
2. The verifier generates independent 32-byte CSPRNG challenge and session
   nonces. Its signed challenge contains an exact schema version, network ID,
   purpose, verifier identity/audience, subject DID, subject key fingerprint,
   session nonce, challenge nonce, issue/expiry times, policy digest, and circuit
   ID. Unknown fields and duplicate JSON keys are rejected.
3. Canonicalize the challenge body using the repository's existing JCS rules.
   Use integer Unix seconds, fixed lowercase hexadecimal byte fields, and
   canonical string identifiers. Signatures are outside the body being hashed.
   Domain-separate the verifier's Ed25519 signature from all other messages.
4. Define `signal = SHA256("alexandria/personhood-signal/v1\0" || JCS(challenge))`.
   Pass these exact 32 bytes through the pinned SDK signal-hash convention:
   `signalHash = unsigned_big_endian(keccak256(signal)) >> 3`. The verifier
   independently derives the same expected public signal; it never trusts the
   supplied commitment. The hash convention is implemented in upstream
   [core/hash.ts](https://github.com/anon-aadhaar/anon-aadhaar/blob/4dad918761cfb1d7d5ed9918dcd796d0cb23ae82/packages/core/src/hash.ts).
   Cross-language vectors must pin the bytes and result before integration.
5. Generate the proof with the required seed and disclosure flags. The account
   signs a separate domain-separated submission containing the challenge digest
   and the canonical proof/public-signals digest. That signature binds submission
   to possession of the named account key, including the exact proof.
6. The verifier validates the challenge signature, outstanding nonce, audience,
   account signature and key status, policy/circuit IDs, expiration, all public
   signals, and the Groth16 proof. Recheck expiration and policy/key status at
   acceptance so a slow proof cannot cross an expiry boundary unnoticed.
7. In one transaction, consume the outstanding challenge and store the receipt.
   A duplicate submission returns the same receipt only for the same authenticated
   account and identical submission digest; a different submission for the spent
   challenge is rejected. Concurrent requests cannot create two receipts.

Pin the seed through the policy, independently of the subject and challenge.
Changing the seed per account or session would defeat duplicate detection within
the declared scope. Rotating a circuit, policy, or seed must have an explicit
migration rule; a new policy must not silently create a second uniqueness domain.

An account key change during an outstanding challenge requires a new challenge.
Future credential transfer or account recovery requires a separate policy; a new
proof must not silently transfer ownership or overwrite a previous binding.

## Verification boundary and privacy

Keep acceptance logic separate from the native prover. A verifier-only Rust
module should take bounded decoded input, an explicit policy, account-key
resolution, and trusted verification time. Storage supplies the transaction that
consumes the challenge. The client-side success screen is never an authorization
decision for a remote service.

Require exactly nine public signals in the pinned order. Reject noncanonical
integers, negative values, extra signals, and scalar/coordinate values outside
their respective fields rather than silently reducing them. Use a reviewed
Groth16 verifier with the required curve, subgroup, and point-validation checks.
Cap envelope size, decoding depth, verification concurrency, and request rate
before expensive cryptography. Malformed, expired, untrusted, replayed, and
temporarily unverifiable inputs need distinct internal outcomes; none grants
authority.

Store the receipt in the account's encrypted profile storage. It should record
the subject, network, policy/circuit identity, verifying authority, issue/expiry
times, submission digest, and result type. A local diagnostic receipt must be
visibly distinct from an authority-signed receipt and must not enter the VC
authority or qualification path.

Do not persist document bytes, witness data, or extracted attributes by default.
Erase temporary input after success, failure, cancellation, or worker death.
Synthetic lab proof retention is a developer convenience, not a production
retention policy. Any authoritative verifier's minimal replay/duplicate ledger
needs its own retention and access policy.

No automatic gossip, public profile badge, or public nullifier publication.
An authoritative verifier necessarily sees public signals including the scoped
nullifier and timestamp and can correlate repeated submissions within that
scope. Document that disclosure before real input is accepted. A local receipt
does not establish network-wide uniqueness; partitions and competing verifiers
require a canonical shared registry/consensus design before any such guarantee.

## Next implementation slice and acceptance tests

The synthetic challenge/submission protocol, pure Groth16 verification and
transactional store are implemented; see [the lab guide](personhood-lab.md)
for host and OnePlus results. The separate real-document policy foundation has
six test groups covering pinning, malformed configuration, issuer status,
expiry boundaries and signal restrictions. It is not yet connected to a real
document parser or receipt issuance path.

Next, complete the production certificate review, pin a reviewed policy and
implement bounded real-input parsing with private temporary-data cleanup. Reuse
the tested account/challenge protocol with the real policy, and recheck policy
at receipt acceptance. Keep the real-input selector disabled until these gates
pass. Use an injected test
clock in unit tests; do not expose a caller-controlled verification time in
production IPC or a service API.

The acceptance suite must cover:

- Valid proof, account signature, policy, and challenge produce one diagnostic
  receipt; the existing account's permissions remain unchanged.
- Wrong issuer, test/production policy confusion, changed verifier key, wrong
  circuit, missing/revoked trust data, wrong seed, and disclosed attributes fail.
- Another account, key, audience, purpose, network, session, policy digest, or
  challenge cannot reuse the proof, including a correctly signed submission by
  the wrong account.
- Expired challenges, future/old documents, clock boundaries, and revocation
  between proof generation and transaction commit fail.
- Replay, concurrent replay, process crash at the transaction boundary, and
  retry after an uncertain response cannot issue conflicting receipts.
- Mutated proof points/signals, field aliases, malformed points, duplicate JSON
  keys, and oversized inputs fail before acceptance.
- Cancellation, backgrounding, profile locking, and process death leave no
  usable partial receipt or retained real-input material.

Before real input, finish certificate and circuit-format validation, choose the
policy lifetime and stable seed, specify real-input retention/cleanup, and test
the integrated path. Release delivery and pilot freshness limits are agreed.
External relying parties or verifier operators need a separate authority design.
Account gating or global uniqueness would be an additional design decision,
not a side effect of this private receipt flow.
