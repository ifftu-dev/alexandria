# DHT Username Registry

**Status:** Implemented (phases 1–4)
**Protocols:** Kademlia records · `/alexandria/username-reg/1.0` · Cardano metadata label 1698

First-come-first-served `@username → DID` bindings, verifiable by any
node. A pure DHT cannot give consensus-grade uniqueness (no global
ordering), so the registry layers guarantees: best-effort DHT claims,
hardened by relay receipts, made trustless by batched Cardano anchors.

## The claim

```
UsernameClaim {
  version:    1,
  username:   "ada_99",              // [a-z0-9_]{3,32}, normalized
  did:        "did:key:z6Mk…",
  claimed_at: unix_ts,               // self-asserted (tier 0)
  sig:        Ed25519(did key) over "alexandria-username-claim-v1|u|did|t",
  receipts?:  [{ relay_peer_id, received_at, sig }], // tier 1 (one per relay)
  anchor?:    { tx_hash, slot },                     // tier 2
}
```

(A legacy single `receipt` field is folded into `receipts` on read and
mirrored back for older builds.)

`did:key` embeds the public key, so claims verify offline with no PKI.
Forging a claim *for someone else's DID* is impossible; the attack
surface is competing claims, resolved by deterministic ordering.

- DHT key: `SHA256("alexandria:username:v1:" + username)`.
- Local cache: `username_claims` (migration 055) — the verified winner
  per username; `anchor_verified` (migration 056) gates tier 2.
- Code: `domain::username_claim`, `commands::username_registry`,
  `p2p::username_reg`, `cardano::username_anchor`.

## Conflict ordering (identical on every node)

1. **Tier** — anchored (2) > receipted (1) > bare (0).
2. **Time** — anchor slot / the **upper median** of verified receipt
   times / `claimed_at`. The median means a minority of dishonest
   relays can neither backdate a friend nor stall a victim — no single
   relay's clock decides anything.
3. **Lexicographic DID** as final tiebreak.

Partitions converge after heal; the deterministic loser sees a Home
banner and a rename flow (`set_username`, Settings → Account).

Tier hygiene at resolution: receipts not signed by a configured relay
([`p2p::discovery::relay_peer_ids`]) are stripped; anchors count only
after this node has verified the digest on-chain (`anchor_verified`).
A forged receipt or tx hash cannot inflate a claim's tier.

## Relay receipts (`/alexandria/username-reg/1.0`)

The relay verifies a claim's signature, applies first-come-first-served
per username, and countersigns
`"alexandria-username-receipt-v1|claim_sig|received_at|relay_peer_id"`
with its stable keypair. Same-DID refreshes re-issue the original
first-seen time — republishing never resets priority. Receipts and a
mirror of inbound DHT records persist in sqlite under
`RELAY_DATA_DIR`; mirrored records warm-load into the kad store at
boot, so the registry survives relay restarts.

Claims gather receipts from **every** configured relay (receipt
diversity). Trust anchor: the relay's ed25519 PeerId embeds its public
key — no key distribution needed, but relays **must** run with a
stable `RELAY_SEED`.

The relay also serves `GET /username/:name` on its HTTP port (9090):
signup runs before any wallet (and therefore P2P identity) exists, so
availability checks fall back to these endpoints — all relays are
queried, and a name is taken if any relay says so.

## Batched Cardano anchoring (label 1698)

One metadata-only tx anchors up to 80 claim digests
(`blake3(claim.sig)`) — **~0.011 ADA per username** vs ~0.18 for
individual anchoring:

```
{ 1698: { "v": 1, "c": [ { "u": "<username>", "h": "<digest>" }, … ] } }
```

`cardano::username_anchor::tick` runs in the scheduler: any node with
a funded wallet + Blockfrost key batches unanchored claims
altruistically (claims are public; an anchor only timestamps them),
attaches anchors, and republishes tier-2 claims to the DHT. The same
tick verifies foreign-anchored claims arriving via the DHT and demotes
any whose digest is absent from the anchoring tx.

## Flows

- **Signup** — live availability (cache → DHT → relay):
  *available / taken / can't-verify-offline* (offline signup proceeds
  with a warning; the claim publishes on next P2P start).
- **Claim** — `claim_username`: cache → DHT put → relay receipt →
  enriched republish. Idempotent; refreshes keep the original time.
  Re-run on every `p2p_start` since kad records expire (~36 h).
- **Lookup** — `/u/@name` resolves the registry claim first; the
  winning claim's DID is authoritative for the profile fetch, so a
  malicious node can't answer for a handle it doesn't hold.
- **Rename** — `set_username` validates + checks availability, updates
  the identity row, claims fresh. The old claim ages out of the DHT.

## Known limitations

- **No release tombstones yet** — an abandoned handle frees in the DHT
  at record expiry but stays reserved at the relay indefinitely.
- Nodes without Blockfrost cannot verify anchors locally and rank such
  claims by their receipt tier until a verifying node's view reaches
  them.
- Anchor `slot` is the tip estimate at submission, not the inclusion
  slot — fine for ordering, where tier dominates.
- The relay is a trusted first-seen oracle (tier 1). Anchoring (tier 2)
  removes that trust for anyone willing to wait a batch interval.

## Operator checklist

1. `relay_data` volume per region (CI provisions it; `bom` currently
   refuses volume creation, so Mumbai runs an ephemeral store —
   issued receipts stay valid there, only refusal history resets).
2. `RELAY_SEED` set as a Fly secret — stable countersigning identity.
3. Batcher node: `BLOCKFROST_PROJECT_ID` + funded payment address.
4. Deploys are tag-triggered (`v*`) via the relay repo's CI, which
   scales each region to one machine (the receipt store is
   single-writer sqlite).
