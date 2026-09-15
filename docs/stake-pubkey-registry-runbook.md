# Stake-Pubkey Registry — Pre-Launch Runbook

Operational steps for rotating the founder trust roots and signed bootstrap
snapshot, plus repeating the preprod registration smoke test. The original
three-key ceremony, signed snapshot, and first real preprod registration are
complete. Founder public keys now live in the versioned preprod network profile,
not a registry-module constant.

Design context: [`stake-pubkey-registry.md`](./stake-pubkey-registry.md).

---

## 1. Founder keypair ceremony

### 1.1 Prerequisites

- Three founder devices. Each device should be **air-gapped or
  network-disconnected** for the duration of the keygen + first sign
  steps — these private keys never need internet access.
- A shared (online) location to publish the public keys: typically a
  Git PR against this repo.

### 1.2 Generate one keypair per founder

On each founder's machine (after `git clone` and a successful
`cargo check`):

```bash
cargo run --manifest-path src-tauri/Cargo.toml \
    --example snapshot_keygen -- --out ./founder_<name>.sk
```

The command:

- Writes the 32-byte raw Ed25519 secret key to `./founder_<name>.sk`
  with file mode `0600`.
- Prints the matching public key hex to stdout. Copy this hex.

**Operational rules:**

- The `.sk` file MUST NOT enter Git, S3, Dropbox, iCloud, Bitwarden's
  shared vaults, etc. Each founder keeps their own.
- A second air-gapped backup (printed paper, hardware-encrypted USB,
  etc.) is recommended.
- Re-running with the same `--out` fails by design (`create_new`) so
  you cannot accidentally overwrite.

### 1.3 Update the network profile founder keys

Open `src-tauri/resources/networks/preprod.json` and replace the
`stake_registry_founder_keys` entries with the three public-key hex strings
from step 1.2. Keep unique stable signer IDs. Snapshot verification counts
distinct configured public keys; signer labels do not substitute for a valid
signature.

### 1.4 Author the first real snapshot

Edit `src-tauri/resources/bootstrap_registry.json` to list the initial
committee. Each entry binds a Cardano stake address to the Ed25519
public key it will use to sign privileged-topic gossip envelopes
(taxonomy / governance / Sentinel priors / goal templates / question banks).
The reserved plugin-attestation topic also passes this envelope gate for wire
compatibility, but its messages grant no application authority:

```json
{
  "version": 1,
  "issued_at": "2026-MM-DDTHH:MM:SSZ",
  "entries": [
    {
      "stake_address": "stake1u…",
      "public_key_hex": "32-byte hex",
      "valid_from": 1748131200,
      "valid_until": null,
      "on_chain_tx": null
    }
  ],
  "signatures": []
}
```

Leave `signatures: []`. Step 1.5 fills them in.

### 1.5 Multisig-sign the snapshot

Each founder, on their own machine, runs:

```bash
cargo run --manifest-path src-tauri/Cargo.toml \
    --example snapshot_sign -- \
    --in  src-tauri/resources/bootstrap_registry.json \
    --out src-tauri/resources/bootstrap_registry.json \
    --key ./founder_<name>.sk \
    --signer founder_<name>
```

The tool reads the snapshot, signs the canonical JCS bytes of
`(version, issued_at, entries)`, and appends one signature object.
Running the same `--signer` label twice (e.g. after editing the
snapshot and re-signing) idempotently replaces the prior signature
rather than duplicating it.

At least 2 of 3 founders must sign. After two have committed their
signatures back to the branch, the third is optional but recommended.

### 1.6 Verify locally

Anyone with the merged branch can confirm the snapshot meets quorum:

```bash
cargo run --manifest-path src-tauri/Cargo.toml \
    --example snapshot_verify -- \
    --in src-tauri/resources/bootstrap_registry.json
```

Expected output:

```
OK: <N> entries, <M> signatures verified against embedded network-profile founder keys
```

If verification fails on `SnapshotQuorum`, one signature is missing
or invalid. If it fails on `VerifierKey`, the corresponding network-profile
founder key is malformed or does not match the signing key.

### 1.7 Land the change

Compute SHA-256 over the final signed `bootstrap_registry.json` bytes and set
that exact lowercase digest at
`signed_bootstrap_registry_identity.sha256` in the same network profile. Merge
both files together. The release build embeds both resources, verifies their
identity before opening profiles, and seeds each fresh profile database from
the signature-verified snapshot.

To rotate a founder key later: regenerate it, update the network profile,
re-sign the snapshot to quorum under the new configured set, update the
snapshot digest, and ship those changes in one release. Old releases continue
using their embedded profile and snapshot until upgraded.

---

## 2. Preprod smoke test

End-to-end exercise of:

- `build_registration_tx` (PR B-2) → real `stake_pubkey_registration`
  UTxO submitted to preprod via Blockfrost
- `BlockfrostClient::get_tx_cbor` + witness verification (PR B-1)
- `BlockfrostFetcher::fetch` → confirms the entry roundtrips

### 2.1 Prerequisites

- A funded preprod wallet. The `keys/` directory + `.env` from
  `alexandria-mark2` already carry a funded deployer wallet that
  works here.
- A preprod Blockfrost project id. The smoke test reads it from the
  `BLOCKFROST_PROJECT_ID` env var (production code prefers the
  `cardano.blockfrost_project_id` setting and falls back to env — see
  `cardano::blockfrost::resolve_project_id`).
- The wallet's 24-word BIP-39 mnemonic (Mode A) or `.skey` files
  (Mode B). Export temporarily; never commit.

### 2.2 Run the round-trip

Two key-source modes:

**Mode A — BIP-39 wallet (preferred for Alexandria-derived wallets):**

```bash
BLOCKFROST_PROJECT_ID=<preprod project id> \
ALEXANDRIA_TEST_MNEMONIC="word1 word2 … word24" \
    cargo run --manifest-path src-tauri/Cargo.toml \
    --example preprod_registration_roundtrip
```

**Mode B — Raw Cardano-CLI `.skey` files (used by the mark2 treasury):**

```bash
BLOCKFROST_PROJECT_ID=<preprod project id> \
ALEXANDRIA_TEST_PAYMENT_SKEY=./keys/treasury.skey \
ALEXANDRIA_TEST_STAKE_SKEY=./keys/treasury-stake.skey \
ALEXANDRIA_TEST_ADDRESS=addr_test1q... \
    cargo run --manifest-path src-tauri/Cargo.toml \
    --example preprod_registration_roundtrip
```

The `.skey` files must be `PaymentSigningKeyShelley_ed25519` /
`StakeSigningKeyShelley_ed25519` JSON wrapping a 32-byte CBOR-tagged
secret (the format Cardano CLI emits).

Optional environment variables (apply to both modes):

| Variable                    | Default                              | Effect |
| --------------------------- | ------------------------------------ | ------ |
| `ALEXANDRIA_TEST_PUBKEY`    | wallet payment public key            | Pubkey to bind. Override if the registry should point at a different gossip-envelope key. |
| `ALEXANDRIA_TEST_VALID_SECS`| `31_536_000` (1 year)                | Length of `valid_until - valid_from`. The special value `0` writes `valid_until = 0` on chain, which the parser maps to "open-ended" (no expiry) — use this for the bundled snapshot's fixture binding so the runbook doesn't need yearly renewal. |

The example logs each step; expected sequence:

1. Wallet derivation: prints stake address + payment address.
2. Builds the tx; prints the locally-computed hash + tx size.
3. Submits via Blockfrost. Prints
   `https://preprod.cardanoscan.io/transaction/<hash>`.
4. Polls `is_tx_confirmed` every 10s (up to 4 minutes).
5. Runs `BlockfrostFetcher::fetch()` against the live script address.
6. Asserts the entry roundtrips with matching `stake_address`,
   `public_key_hex`, and `on_chain_tx`.

On success:

```
✓ end-to-end smoke test PASSED
  stake_address : stake_test1u…
  public_key    : <hex>
  valid_from    : 1748131140
  valid_until   : 1779667140
  on_chain_tx   : <submitted hash>
```

### 2.3 Failure modes

| Symptom | Likely cause | Fix |
| ------- | ------------ | --- |
| `build_registration_tx` `NoUtxos` / `InsufficientFunds` | Wallet not funded on preprod | Top up via the [preprod faucet](https://docs.cardano.org/cardano-testnets/tools/faucet/) |
| `submit_tx` 400 from Blockfrost | Plutus-era encoding mismatch or stake key not signing | Inspect the tx CBOR; confirm both vkey witnesses are present |
| Confirmation never lands | Network congestion or wrong network | Check the Cardanoscan link manually |
| Fetcher returns 0 entries | Blockfrost UTxO index hasn't caught up | Re-run after a minute; Blockfrost indexes lag tx confirmation by a few seconds |
| Fetcher returns entries but ours is missing | Witness check rejected it | `tx_witnesses_include_stake_key` returned `NoMatchingWitness`; means the stake key signature was not in the witness set (bug in `build_registration_tx`) |

### 2.4 Cost

The registration tx locks `REGISTRATION_UTXO_LOVELACE = 3 ADA` at the
script address forever (the validator fails on every spend purpose).
Plan for one funded UTxO per stake-key rotation. Test preprod ADA is
free from the faucet, so this is only a planning concern for
mainnet.

---

## 3. Current status and remaining boundary

- Real founder keys live in `preprod.json`; the signed bootstrap snapshot and
  its SHA-256 resource identity ship with the build.
- The preprod registration round trip has been confirmed against live Cardano.
- The app is intentionally preprod-only. A mainnet release requires a separate
  reviewed network profile, immutable profile separation, and coordinated
  protocol/service deployment; changing the Cardano enum alone is insufficient.
