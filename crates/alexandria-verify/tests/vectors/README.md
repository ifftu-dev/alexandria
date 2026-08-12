# Alexandria credential test vectors

Signed credentials with known-correct verification outcomes. Written so that an
independent verifier — in any language — can be checked against this one and
shown to agree.

Nothing here depends on Alexandria being reachable, running, or involved. Each
file is self-contained: the credential, whatever state verification needs, the
verification time, the policy, and the expected result. There is no network
access anywhere in the verification path.

## What a verifier needs

Two primitives, and no more:

- **JCS** — JSON Canonicalization Scheme, [RFC 8785](https://www.rfc-editor.org/rfc/rfc8785)
- **Ed25519** — [RFC 8032](https://www.rfc-editor.org/rfc/rfc8032)

In particular you do **not** need JSON-LD tooling. Canonicalization is JCS over
the credential's JSON document, not RDF Dataset Canonicalization. `@context` is
declarative — it is covered by the signature like any other field, and nothing
expands or dereferences it. See §14.12a of the protocol specification.

## The algorithm, in short

1. Copy the credential and set `proof.jws` to the empty string.
2. Canonicalize that copy with JCS. These are the signing bytes.
3. Split `proof.jws` on `.` — it is a *detached* JWS, so it has the form
   `<protected-header>..<signature>` with an empty middle segment. Both outer
   segments are base64url, unpadded.
4. Build the signing input as
   `<protected-header> || "." || <signing bytes>` — the **raw** canonical bytes,
   not base64url-encoded.
5. Verify that Ed25519 signature against the issuer's public key.

   Step 4 is the one to read twice. The protected header is
   `{"alg":"EdDSA","b64":false,"crit":["b64"]}`, which is
   [RFC 7797](https://www.rfc-editor.org/rfc/rfc7797) unencoded-payload mode:
   with `b64:false` the payload is appended raw rather than base64url-encoded.
   A verifier that assumes ordinary JWS will base64url the canonical bytes,
   produce a different signing input, and reject every credential ever issued —
   with no clue as to why, because every other check passes.
6. Apply the remaining checks: expiry, subject binding, status list.

## Resolving the issuer key

`did:key` is self-resolving: the public key is embedded in the identifier, so no
lookup is required in the common case. Decode the multibase (`z`, base58btc),
strip the `0xed 0x01` multicodec prefix, and the remaining 32 bytes are the
Ed25519 public key.

If the vector supplies `store.keyRegistry`, prefer an entry whose
`[validFrom, validUntil)` window contains the verification time. That is what
makes a credential signed before a key rotation still verify afterwards — see
`08-rotated-issuer-key.json`, where self-resolution alone gives the wrong key
and fails.

## What a bare credential cannot tell you

`did:key` is self-resolving, so most credentials verify with nothing but the
document in front of you. Two cases need more, and both are inherent rather than
defects:

- **A rotated issuer key** — self-resolution returns the pre-rotation key, so a
  credential signed after a rotation needs the registry entry covering the
  verification time. An identifier that embeds one key cannot embed its
  successors. See `08-rotated-issuer-key.json`.
- **Revocation** — a credential names its status list; it does not carry it.

Neither requires contacting Alexandria. Both require having been given the data,
which is what a credential *bundle* is for: it carries the key registry and the
status lists next to the credentials, and verifies entirely offline.

## Status lists

`credentialStatus.statusListIndex` is a bit index into the list named by
`statusListCredential`. Bit *n* is byte `n / 8`, bit `n % 8`, **little-endian
within the byte** — so index 9 is byte 1, mask `0x02`. Getting this backwards is
the single most common interoperability bug, which is why
`07-revoked.json` exists.

A status list the verifier does not have is not evidence of revocation. Absence
means "not known to be revoked", never "revoked".

## File format

```jsonc
{
  "description": "what this case asserts, in a sentence",
  "verificationTime": "2026-06-01T00:00:00Z",   // the `now` to verify against
  "policy": { ... },                            // acceptance rules
  "credential": { ... },                        // the signed credential
  "store": {                                    // optional; empty = no local context
    "keyRegistry":  [ { "did", "keyId", "publicKeyHex", "validFrom", "validUntil" } ],
    "statusLists":  { "<list id>": "<hex bytes>" },
    "suspended":    { "<credential id>": "<until>|null" },
    "superseded":   [ "<credential id>" ]
  },
  "expect": { ... }                             // the required result
}
```

Compare at least `validSignature`, `issuerResolved`, `expired`, `revoked`,
`subjectBound` and `acceptanceDecision`. The booleans matter independently of
the decision: policy changes the decision, never the facts. `09` and `05` are
the same expired credential, and differ only in what the policy does about it.

## The suite

| Vector | Asserts |
|---|---|
| `01-valid` | The happy path: signature, self-resolution, acceptance |
| `02-tampered-payload` | A claim altered after signing. The sharpest test of a JCS implementation — a verifier that canonicalizes differently may wrongly accept |
| `03-wrong-signing-key` | Signed by a key the issuer DID does not name |
| `04-malformed-jws` | `proof.jws` is not a JWS. Must reject, not error |
| `05-expired` | `validUntil` precedes the verification time |
| `06-non-did-subject` | Subject id is not a DID, so nothing is bound |
| `07-revoked` | Status-list bit set. Pins the bit-order convention |
| `08-rotated-issuer-key` | Signed with a rotated key; only the registry resolves it |
| `09-expired-permissive-policy` | Same expired credential, policy does not reject. `expired` stays true |
| `10-type-not-allowed` | Every cryptographic check passes; policy still rejects |

Every column of that matrix has both outcomes present, so an implementation
cannot pass by hardcoding any single check.

## A worked implementation

`independent-verifier.mjs` in this directory is a complete verifier written from
this document alone — no Alexandria library, nothing but Node's standard crypto,
JCS implemented by hand. It passes all ten vectors.

```sh
node independent-verifier.mjs
```

It is here as evidence, not as shipped code. If it stops passing, either this
document has drifted from the format or the format changed without the document
being updated — and both are bugs in the promise that anyone can verify.

## Regenerating

```sh
ALEXANDRIA_REGENERATE_VECTORS=1 cargo test -p alexandria-verify --test vectors
```

Signing keys derive from fixed byte patterns, so regeneration is deterministic
and produces byte-identical files. If a regeneration changes a file, the wire
format moved — which is worth seeing in a diff rather than discovering in
somebody else's implementation.

The keys here are published on purpose and secure nothing.
