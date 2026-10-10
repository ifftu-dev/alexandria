# Sentinel Model Runbook

> Operational playbook for the bundled Sentinel paste classifier.
> Audience: maintainers who retrain or roll back the model.
> Companion docs: [sentinel.md](sentinel.md),
> [sentinel-federation.md](sentinel-federation.md).
>
> **Backend architecture note.** All ML inference and training runs in
> the Rust backend (`tract` for ONNX inference, `candle` for per-user
> autoencoder and CNN training). The frontend only buffers raw events
> and forwards them via Tauri IPC. The paste-classifier ONNX is embedded
> at compile time via `include_bytes!` from
> `src-tauri/resources/sentinel/paste-v1.onnx`, and it is the only model
> the classifier parses: no command accepts replacement weights. Per-user
> weights persist in the `sentinel_user_models` SQLite table
> (SQLCipher-encrypted).
>
> The earlier community prior library — Sentinel DAO proposals,
> ratification, runtime weights replacement, the kill switch and the
> version blocklist — is deleted. A model reaches users only through an
> app release.

## Quick reference

| Action | Where | Tool |
|--------|-------|------|
| Generate synthetic training data | `cli/` | `cargo run -p alexandria -- synth-sentinel ...` |
| Train + export ONNX | `tools/sentinel-train/` | `python train.py` |
| Verify holdout gate | `tools/sentinel-train/` | `python eval.py` |
| Ship a model | `src-tauri/resources/sentinel/` | Replace the artifact and its SHA-256 lockfile in a release |
| Check what a client runs | Tauri app or IPC | `sentinel_paste_classifier_info` |
| Re-export the liveness model | `scripts/sentinel/` | `python export-minifasnet.py <Silent-Face-Anti-Spoofing checkout> <out.onnx>` (see Procedure 4) |
| Enable device attestation | Apple developer portal, Play Console, `preprod.json` | See Procedure 5 |

---

## Procedure 1: Retrain and ship the bundled classifier

**Prerequisites:**
- A reproducible Python env (`tools/sentinel-train/.venv` after `pip install -r requirements.txt`).
- The `alexandria` CLI builds (`cargo build -p alexandria`).

### Step 1 — Generate training + holdout corpora

```bash
cd alexandria
cargo run -p alexandria -- synth-sentinel generate-all     --out-dir tools/sentinel-train/priors
cargo run -p alexandria -- synth-sentinel generate-holdout --out-dir tools/sentinel-train/holdout
```

Output is deterministic per seed; the golden-hash test in
`cli/src/synth/generators.rs` will catch any drift from
`SYNTH_VERSION = "v2"`.

### Step 2 — Featurize, train, eval

```bash
cd tools/sentinel-train
.venv/bin/python featurize.py --in priors   --out train.npz
.venv/bin/python featurize.py --in holdout  --out holdout.npz
.venv/bin/python train.py    --train train.npz   --out paste-vNEXT.onnx --epochs 30
.venv/bin/python eval.py     --model paste-vNEXT.onnx --holdout holdout.npz --out eval.json
```

`eval.py` exits non-zero if the gate fails:

- `macro_tpr >= 0.92`
- `macro_fpr <= 0.03`
- `per_label.paste_macro.tpr >= 0.98`
- `per_label.llm_paste_edit.tpr >= 0.85`

Do **not** proceed if any gate fails. Investigate before re-running.

### Step 3 — Replace the bundled artifact

```bash
cp paste-vNEXT.onnx ../../src-tauri/resources/sentinel/paste-v1.onnx
cd ../../src-tauri/resources/sentinel
shasum -a 256 paste-v1.onnx > paste-v1.onnx.sha256
```

Run the classifier tests, which score every attack archetype against
the embedded model and fail if any stops out-scoring the human baseline:

```bash
cargo test -p alexandria-node sentinel::paste_classifier
```

### Step 4 — Release

Commit the artifact, lockfile and `eval.json` numbers together, then ship
an app release. After updating, `sentinel_paste_classifier_info` reports
`{ source: 'bundled', version: 'bundled-v1' }`; the model bytes are the
ones in the release.

---

## Procedure 2: Respond to a bad classifier

Use when the paste classifier produces unacceptable false positives.

1. AI signals are advisory and off by default
   (`sentinel.ai_scoring_enabled`). Confirm affected profiles have not
   turned them on; if they have, the per-signal
   `sentinel.paste_classifier_enabled` toggle removes only the paste
   signal.
2. Restore the previous artifact and lockfile from git history, rerun
   the Procedure 1 Step 3 tests, and ship an app release.

---

## Procedure 3: Bump the synthetic generator

When you legitimately change a generator distribution (e.g. add a new
attack class), the golden hashes will fail. To bump cleanly:

1. Edit `cli/src/synth/generators.rs` distributions.
2. Bump `SYNTH_VERSION` in `cli/src/synth/blob.rs` (currently `"v2"`) to the next version (e.g. `"v3"`).
3. Update the `golden_hashes_match_synth_v2` test in `generators.rs`:
   - Rename it to match the new version (e.g. `golden_hashes_match_synth_v3`).
   - Update the version assertion (e.g. `SYNTH_VERSION == "v3"`).
   - Regenerate hashes using the recipe in the test doc-comment.
4. Run `cargo test -p alexandria synth` — must pass.
5. Retrain the classifier (Procedure 1), because the training corpus
   distribution has shifted.

---

## Procedure 4: Re-export the liveness model

The bundled `minifasnet-v2-80.onnx` is the Apache-2.0 `2.7_80x80_MiniFASNetV2.pth` from minivision-ai's Silent-Face-Anti-Spoofing with softmax folded in. To re-export (new upstream weights, new opset, or a tract parser change):

```bash
git clone --depth 1 https://github.com/minivision-ai/Silent-Face-Anti-Spoofing /tmp/sfas
uv venv --python 3.12 /tmp/exportenv && uv pip install --python /tmp/exportenv/bin/python torch onnx
/tmp/exportenv/bin/python scripts/sentinel/export-minifasnet.py /tmp/sfas src-tauri/resources/sentinel/minifasnet-v2-80.onnx
cd src-tauri/resources/sentinel && shasum -a 256 minifasnet-v2-80.onnx > minifasnet-v2-80.onnx.sha256
```

The script prints the softmax for an all-zero and an all-128 input; paste both into `REF_ZEROS` / `REF_MID` in `src-tauri/src/sentinel/liveness.rs` and run `cargo test -p alexandria-node sentinel::liveness`. The cv2-pipeline parity test needs no update unless the preprocess changes. Commit artifact, lockfile and reference values together; CI verifies the lockfile.

## Procedure 5: Enable device attestation

`device_attested` needs one-time platform setup; until it is done, sessions stay `local` and nothing fails.

**iOS (App Attest).**

1. In the Apple developer portal, enable the **App Attest** capability on the App ID `org.alexandria.node` (team `VLMNL3V44U`).
2. The project already carries the entitlement `com.apple.developer.devicecheck.appattest-environment = production`; Xcode's automatic signing picks up the regenerated profile.
3. Verify on a physical device (the simulator cannot attest): start an assessment and open **Sentinel Dev**; the `attestation` row reads `device_attested`. Debug builds attest in the development environment, which only debug builds accept.

**Android (Play Integrity).**

1. The app must be listed in Play Console (an internal testing track is enough). Sideloaded builds get `CLOUD_PROJECT_NUMBER_IS_INVALID` or an unrecognised-app verdict and stay `local` by design.
2. Play Console → App integrity → Play Integrity API → **Link a Cloud project**.
3. On the same page, under response encryption, choose **Manage and download my response encryption keys** and download them.
4. Paste the two base64 values into `src-tauri/resources/networks/preprod.json`:

   ```json
   "play_integrity_response_keys": {
     "decryption_key_b64": "<AES key>",
     "verification_key_b64": "<EC public key>"
   }
   ```

5. `cargo test -p alexandria-node --lib -- network_profile` refuses a key that is not 32 bytes or not a P-256 public key. The keys let a client read verdicts, not forge them.

## Threat-model checklist

Before shipping a retrained classifier, confirm:

- [ ] `eval.json` came from `tools/sentinel-train/eval.py` run with
      `--threshold 0.5` against the released artifact bytes. Lower
      thresholds invalidate the gate assumptions.
- [ ] Training script `train.py` was run against the generated corpus,
      not a private superset.
- [ ] No face-related features in the training pipeline. (Per
      [sentinel-federation.md](sentinel-federation.md) decision 2 — face
      is never federated.)
- [ ] Holdout was generated with `synth-sentinel generate-holdout`
      (seed-base 100000+) and not reused as training data.
- [ ] The SHA-256 lockfile matches the committed artifact.

---

## Incident-response template

When the production classifier behaves badly, file an incident note
with these fields:

```
Date:             <ISO 8601>
Severity:         P1 / P2 / P3
Symptom:          (e.g. FPR spike in iOS users after a model release)
Affected release: app version and artifact SHA-256
Action taken:     (per-signal opt-out guidance / artifact rollback release)
Root cause:       (TBD on first save)
Followups:        (retrain / threshold tune / new attack class / etc)
```
