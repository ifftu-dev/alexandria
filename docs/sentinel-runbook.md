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
