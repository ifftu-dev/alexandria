# 0007. A `did:key` never rotates

Status: implemented
Date: 2026-11-16

## Context

[0001](0001-vc-first-credentials.md) makes `did:key` the subject and issuer identity. A personal key rotation overlay had been half-built on top of it:

- nothing could trigger a rotation;
- the verifier chose the issuer key by verification time, so a rotation would have invalidated every credential signed before it;
- the inbound rotation announcement recorded a rotation without the new key;
- a verifier outside Alexandria resolves a `did:key` from the identifier alone and ignores all of it, so Alexandria and standard tooling would disagree about the same credential.

## Decision

A `did:key` resolves to the key in its identifier, always. This amends the identity row of [0001](0001-vc-first-credentials.md).

- The rotation function, the rotated-issuer constructor, validity windows, the rotated-by link and rotation announcements are deleted, in the app and in `alexandria-verify`. A registry row can no longer change how a `did:key` resolves, and the verifier never consults the store for one.
- The key registry remains only for identifiers that are not self-resolving; a row without a key cannot be written. Bundles stop exporting rotation history. DID announcements on `vc-did` register nothing.
- Organisations rotate by editing the key list in their domain's DNS TXT descriptor. A credential's signature always verifies under its key; whether an organisation backs that key is a separate, caller-supplied check ([0008](0008-hosted-organisation-issued-credentials.md)).

## Consequences

- Alexandria's verification result for a `did:key` issuer matches any standard Data Integrity verifier's.
- No dormant code path in the published verifier can invalidate earlier credentials.
- A learner whose key is lost or compromised starts a new identity. Identity recovery needs its own design.
- Removing a key from an organisation's domain withdraws backing from everything that key signed. Preserving backing across a key's removal (for example, by proving it was listed when anchored) is not designed.
