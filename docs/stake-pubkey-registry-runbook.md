# Stake-Pubkey Registry — Pre-Launch Runbook

Operational steps for the two remaining items on PR B before public
launch:

1. Replace the placeholder `SNAPSHOT_VERIFIERS` with real founder keys
   and produce the first signed `bootstrap_registry.json`.
2. Smoke-test `build_registration_tx` + `BlockfrostFetcher` end-to-end
   against Cardano preprod.

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

### 1.3 Update `SNAPSHOT_VERIFIERS`

Open `src-tauri/src/p2p/registry.rs`, locate `SNAPSHOT_VERIFIERS`, and
replace the placeholder hex literals with the three pubkey-hex
strings from step 1.2. Commit the change to a feature branch.

```rust
pub const SNAPSHOT_VERIFIERS: &[(&str, &str)] = &[
    ("founder_a", "<hex from founder_a.sk>"),
    ("founder_b", "<hex from founder_b.sk>"),
    ("founder_c", "<hex from founder_c.sk>"),
];
```

Names are advisory — verification just counts how many distinct keys
in this list produced valid signatures.

### 1.4 Author the first real snapshot

Edit `src-tauri/resources/bootstrap_registry.json` to list the initial
committee. Each entry binds a Cardano stake address to the Ed25519
public key it will use to sign privileged-topic gossip envelopes
(taxonomy / governance / Sentinel priors / plugin DAO attestations):

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
OK: <N> entries, <M> signatures verified against SNAPSHOT_VERIFIERS
```

If verification fails on `SnapshotQuorum`, one signature is missing
or invalid. If it fails on `VerifierKey`, `SNAPSHOT_VERIFIERS` in
`registry.rs` was not updated correctly.

### 1.7 Land the change

Merge the branch. The release build will embed the signed
`bootstrap_registry.json` via `include_bytes!` and every fresh node
will seed its `stake_pubkey_registry` from it on first boot.

To rotate a founder key later: regenerate (step 1.2), update
`SNAPSHOT_VERIFIERS` in code, ship a new release, then re-sign + re-
ship a snapshot. Old releases continue working under the old keyset
until upgraded.

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

## 3. After both steps complete

- PR B-4 closed: real founder keys live in `SNAPSHOT_VERIFIERS`, signed
  bootstrap snapshot ships with the build.
- Preprod smoke green: the full pipeline works against live Cardano.
- Mainnet selection (the `Network::Preprod` hardcode in
  `lib.rs::start_node_with_db`'s registry refresh spawn) is now the
  only remaining pre-launch blocker.
