# Default-network demo genesis

The current `preprod` demo uses **temporary single-operator governance**, explicitly
chosen for the demo. Seven founder entries satisfy the protocol format; all seven
key sets belong to the same operator. They do not represent seven independent people.
This is not a production governance launch.

## Public artifact

- Signed canonical document: [`src-tauri/resources/networks/preprod-demo-genesis.json`](../src-tauri/resources/networks/preprod-demo-genesis.json).
- Network binding: [`src-tauri/resources/networks/preprod.json`](../src-tauri/resources/networks/preprod.json), revision 2, `default_governance_anchor`.
- DAO ID: `af785f1d14788f2d43c8ce8e29c15cd82c4c4cd0e3f56d1646df0dd116e0b671`.
- Exact envelope BLAKE3: `44961c11e052776f04f8b851b3a69a5a039b433690fd6fbd59bf4aa18cd23f3e`.

The file contains public keys and signatures only. Keep its exact canonical bytes:
pretty-printing, adding a newline or reordering keys invalidates the bundled digest.
No public HTTPS mirror or retrievable genesis locator has been published by this
change. The bundled flow works offline and needs neither.

The signed name is “Alexandria preprod — temporary single-operator demo governance”.
Its network scope is `preprod`. Each founder signed the identical core with distinct
identity, consensus and governance keys: 21 verified signatures. Rules retain the
protocol's 5-of-7 receipt/outcome thresholds and two-thirds proposal approval;
minimum turnout is one for the demo. The qualification policy accepts
`assessment-credential` evidence from the existing `demo` instructor:
`did:key:z6MkrgAA1UeVKMFvnerXN4S4Z1cTnzY31Adih31whBp9KY5U`.

These signed terms do not create credentials, start a committee, or implement voting.
The activation timestamp is a genesis term, not evidence that services were launched.

## Profile behavior and demo

Creation and mnemonic restoration verify and pin the exact default document in the
new profile's encrypted database before profile services start. Missing documents,
digest/DAO/network mismatches and invalid signatures fail closed. Unlocking an
existing profile never silently adds or replaces trust anchors. Pinning is idempotent;
an earlier valid envelope for the same DAO core retains its original bytes.

Open **Community**. New profiles show **Pinned for this profile**. For an older
profile, review the signed name and **Verified founding trust facts**, then choose
**Accept this default trust anchor** once. Other communities retain the explicit
locator-review, retrieval and pinning flow. Proposal creation, voting, receipts,
certified outcomes and governance application remain outside this demo.

## Verification and key custody

From the app repository:

```sh
cargo run -p alexandria -- --json governance verify-genesis \
  src-tauri/resources/networks/preprod-demo-genesis.json \
  --expected-dao-id af785f1d14788f2d43c8ce8e29c15cd82c4c4cd0e3f56d1646df0dd116e0b671
```

The same `governance verify-genesis` command is included in the bundled CLI and
works outside a repository without unlocking an app profile.

The initial ceremony used `governance create-demo-genesis`, which generates fresh
OS-random Ed25519 keys and refuses to overwrite an existing ceremony. On this Mac,
keys are retained outside the repository in
`~/.alexandria-governance/preprod-demo-20261002/`: directory mode `0700`, secret-file
mode `0600`. They are **unencrypted raw 32-byte signing seeds**, not app-profile keys.
Keep that directory private and back it up in encrypted storage. Never commit,
publish or include it in demo materials. `founder-N-{identity,consensus,governance}.secret`
maps to the Nth canonically ordered member in the accompanying public genesis.

Do not rerun creation to “refresh” this genesis: new keys or changed signed terms
create a different DAO. Replacing this temporary authority requires a separately
reviewed migration; app updates do not overwrite existing profile pins.

Verification covers governance persistence/retrieval, missing or tampered documents,
DAO/network binding, new/restored versus existing profile initialization, fresh-key
generation, owner-only custody and overwrite refusal. UI tests cover pinned status,
explicit acceptance and verification failures; desktop and phone layouts were reviewed.
