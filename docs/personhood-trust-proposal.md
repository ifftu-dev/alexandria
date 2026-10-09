# Personhood issuer trust and account binding — proposal

Status: proposed design, not implemented or activated. The current lab still uses
fixed synthetic input and grants no authority. The user selected a private
verification receipt as the first real-input outcome on 2026-09-30, with no
account permissions change. The protocol and operational details below remain
for review. Account-creation, voting, reputation, and other privileges are out
of scope.

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

Introduce a versioned personhood policy document whose exact canonical bytes and
digest are pinned through the network profile, following the existing
qualification-policy pattern. No new trust fields are being added by this
proposal. A future profile migration must define them explicitly and fail closed
when a required policy is absent or inconsistent.

The policy would contain:

| Field | Proposed rule |
| --- | --- |
| `network_id`, `policy_version`, `policy_digest` | Match the active network and exact reviewed policy bytes. Never take these values from an untrusted proof. |
| `circuit_id`, `verification_key_sha256` | Select a bundled verifier and exact key. Reject caller-supplied keys, URLs, and circuit substitutions. |
| `document_issuers` | Reviewed issuer-key hashes with provenance, validity intervals, and disabled/revoked status. Test keys are a separate synthetic policy and are never production fallbacks. |
| `verification_authorities` | Explicit identities allowed to sign challenges and receipts for this purpose. Document issuers and Alexandria receipt signers are different roles. |
| `nullifier_scope`, `nullifier_seed` | A verifier-selected, fixed network-and-purpose scope; all accepting verifiers must require the exact seed. Do not let clients choose it. |
| `max_document_age_seconds` | Proposed pilot value: 604800 (seven days), subject to product and issuer review. |
| `max_future_skew_seconds` | Proposed pilot value: 300. |
| `challenge_ttl_seconds` | Proposed value: 120, measured by the issuing verifier. |
| `receipt_ttl_seconds` | Proposed ceiling: 86400, also bounded by document freshness and issuer/policy validity. |
| `attributes` | All four disclosure outputs must be zero in this initial flow. No age, gender, postal code, or state claim. |

Existing relay receipt keys are not implicitly authorized for personhood.
Production issuer material must be obtained through a reviewed authoritative
channel and its circuit key hash independently reproduced. We have not established
that operational trust process yet. Unknown keys, unavailable required status,
expired policies, and revoked issuers cannot produce an accepted receipt. A
document timestamp alone is not evidence that a compromised key signed before
compromise; a revoked issuer must not be grandfathered solely on that timestamp.

For the next synthetic implementation, the verifier is local, uses only a test
policy, and issues a clearly marked diagnostic receipt. Local clock and database
state are not an authority against a device owner. A remotely relied-on receipt
requires a separately designated verifier with authoritative time and storage.

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

Continue with synthetic input only. Add policy parsing, canonical challenge and
submission types, pure verification, and a transactional challenge store. Keep
the real-input selector and all account privileges disabled. Use an injected test
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

Before the later real-input milestone, decide the receipt's intended relying
party, the issuer update/revocation process, verifier operators, retention, and
the proposed freshness windows. Account gating or global uniqueness would be an
additional design decision, not a side effect of this receipt flow.
