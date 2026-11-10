# 0005. Credential anchors are batched and paid by an on-chain sponsor

Status: active
Date: 2026-11-10

## Context

[0001](0001-vc-first-credentials.md) limits Cardano to VC integrity anchoring, the completion witness and governance. The first anchoring path wrote one metadata transaction per credential. The fee came from a treasury key read from a local `.env` file, or else from the issuer's wallet. That worked only on a developer machine:

- a shipped app cannot contain the treasury key, because anyone who extracts it can drain the treasury;
- holding the key in a hosted service would add the central server every Alexandria node is meant to work without;
- funding from the issuer's wallet means every learner needs ADA and pays one fee per credential.

A credential that reached anyone else also carried nothing that let their verifier find its anchor.

## Decision

| Aspect | Choice |
|---|---|
| Batching | An issuer's node anchors one Merkle root over its queued credentials' integrity hashes per batch: after six hours, or at 256 credentials. RFC 6962 / RFC 9162 tree with BLAKE3 and domain separation. |
| Anchor record | Label-1697 metadata, version 2: `{root, leaf_count, issuer_did, anchored_from, v: 2}`. The issuer's `did:key` key is a required signer. Version 1 is dropped. |
| Receipts | Each credential gets an anchor receipt (index, audit path, root, transaction). It is a sidecar, never embedded in the credential, and is exported in credential bundles. |
| Fees | The `anchor_sponsor` Plutus V3 validator holds the treasury's anchoring funds across many UTxOs. Any node may spend one within a fee cap, with the rest returned to the script and a per-UTxO rate limit. An m-of-n founder multisig can reclaim the funds. The issuer's wallet pays when no sponsor UTxO is spendable. |
| Anchor content | Stays in metadata for both funding modes. The validator never reads it: Plutus scripts cannot see metadata, and a datum copy would prove nothing. |
| Verification | `alexandria-verify` folds the receipt from the recomputed hash and checks root, transaction, issuer and required signer against the caller's ledger observation. An anchor is never a gate unless the policy requires one. |
| Chain access | Anchoring reaches Cardano only through a `Ledger` port; Blockfrost is its only implementation today. |
| Coordinates | The sponsor's script hash, address, reference UTxO and parameters live in the network profile ([0002](0002-bundled-network-profiles.md)). |

## Consequences

- No build reads a treasury secret to anchor a credential, and anchoring needs no Alexandria server.
- One transaction covers many credentials, so the treasury pays little per credential and the cost of abuse is bounded by the sponsor's parameters.
- Credentials stay "pending" until their batch flushes, up to six hours after issuance.
- A sponsored spend needs collateral from the issuer's own wallet, so a node with no ADA at all still cannot anchor. Covering that waits on a sybil-resistance decision.
- Received credentials show as anchored once their bundle's receipt verifies against an observed transaction. Bundles verified offline never claim an anchor, because a bundle cannot carry the ledger's word.
- The sponsor's parameters change only by reclaim and redeploy until on-chain governance is active.
- Replacing Blockfrost with direct relay submission and Mithril-certified reads is a follow-up that adds a `Ledger` implementation without changing this design.
