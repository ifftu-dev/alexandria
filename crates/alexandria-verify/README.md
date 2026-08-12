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
use alexandria_verify::{NullStore, vc::{VerificationPolicy, verify::verify_credential}};

// NullStore answers "no revocation, no suspension, no supersession" to
// everything. Correct for a self-contained check; wrong if you hold status
// data and forgot to wire it in.
let store = NullStore;
let policy = VerificationPolicy::default();
let result = verify_credential(&store, &credential, "2026-08-13T00:00:00Z", &policy);

assert!(result.valid_signature);
```

Implement `VerificationStore` over whatever you actually have — SQLite, a
credential bundle, Postgres — and the same verification logic runs against it.
`tests/no_io_deps.rs` fails the build if a dependency that reaches the outside
world is ever added.

## Interoperability

`tests/vectors/` holds signed credentials with known-good and known-bad
outcomes, plus `independent-verifier.mjs` — a ~70-line Node implementation
written against the specification rather than against this code. It passes all
ten vectors. If you are writing your own verifier in another language, start
there: the vectors are the contract, and this crate is one implementation of it.

The signing input is **raw payload bytes**, not base64url — RFC 7797 with
`b64:false`. This is the detail most independent implementations get wrong.

## Licence

MIT OR Apache-2.0, at your option.
