# Sentinel Operator Runbook

> Step-by-step operational playbook for the Sentinel paste-classifier
> pipeline. Audience: Sentinel DAO operators and on-call engineers.
> Companion docs: [sentinel.md](sentinel.md),
> [sentinel-adversarial-priors.md](sentinel-adversarial-priors.md),
> [sentinel-federation.md](sentinel-federation.md).
>
> **Backend architecture note.** As of the May 2026 rewrite, all ML
> inference + training runs in the Rust backend (`tract` for ONNX
> inference, `candle` for per-user autoencoder + CNN training). The
> frontend only buffers raw events and forwards them via Tauri IPC.
> The bundled paste-classifier ONNX is embedded at compile time via
> `include_bytes!` from `src-tauri/resources/sentinel/paste-v1.onnx`.
> Per-user weights persist in the `sentinel_user_models` SQLite table
> (sqlcipher-encrypted), not localStorage.

## Quick reference

| Action | Where | Tool |
|--------|-------|------|
| Generate synthetic training data | `cli/` | `cargo run -p alex -- synth-sentinel ...` |
| Train + export ONNX | `tools/sentinel-train/` | `python train.py` |
| Verify holdout gate | `tools/sentinel-train/` | `python eval.py` |
| Propose new model | Tauri app or IPC | `sentinel_propose_prior` |
| Ratify approved proposal | Tauri app or IPC | `sentinel_ratify_prior` |
| Disable classifier entirely | IPC | `sentinel_set_kill_switch` |
| Block a bad version | IPC | `sentinel_blocklist_version` |
| Check what client is using | Tauri app or IPC | `sentinel_get_active_paste_classifier` |

---

## Procedure 1: Ratify a new classifier model end-to-end

**Pre-requisites:**
- You hold a Sentinel DAO voting key.
- A reproducible Python env (`tools/sentinel-train/.venv` after `pip install -r requirements.txt`).
- The `alex` CLI builds (`cargo build -p alex`).

### Step 1 — Generate training + holdout corpora

```bash
cd alexandria
cargo run -p alex -- synth-sentinel generate-all     --out-dir tools/sentinel-train/priors
cargo run -p alex -- synth-sentinel generate-holdout --out-dir tools/sentinel-train/holdout
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

### Step 3 — Pin the artifacts

Compute and store SHA-256 lockfiles:

```bash
shasum -a 256 paste-vNEXT.onnx | awk '{print $1, "paste-v" V ".onnx"}' V=NEXT \
  > paste-vNEXT.onnx.sha256
```

Inside the Tauri app's devtools console (or via a dedicated script):

```js
const weightsBuf = await fetch(`file:///.../paste-vNEXT.onnx`).then(r => r.arrayBuffer())
const weightsCid = await invoke('content_add', { bytes: [...new Uint8Array(weightsBuf)] })

const evalJson = await fetch(`file:///.../eval.json`).then(r => r.text())
const evalCid  = await invoke('content_add', { bytes: [...new TextEncoder().encode(evalJson)] })
```

### Step 4 — Construct + propose the envelope

```js
const evalReport = JSON.parse(evalJson)
const envelope = {
  schema_version: 1,
  model_kind: 'paste_classifier_weights',
  label: 'paste-vNEXT',
  samples: {
    weights_cid: weightsCid,
    eval_cid:    evalCid,
    eval_tpr:    evalReport.macro_tpr,
    eval_fpr:    evalReport.macro_fpr,
    version:     'paste-vNEXT',
  },
  notes: `Trained ${new Date().toISOString().slice(0,10)}`,
}
const envCid = await invoke('content_add', {
  bytes: [...new TextEncoder().encode(JSON.stringify(envelope))],
})

const { proposal_id } = await invoke('sentinel_propose_prior', {
  req: {
    blob_cid: envCid,
    title: 'paste-vNEXT classifier weights',
    description: `TPR=${evalReport.macro_tpr}, FPR=${evalReport.macro_fpr}`,
  },
})
```

### Step 5 — Vote + ratify

Vote via the existing governance UI (`/dashboard/governance`). Once the
proposal reaches `status='approved'`:

```js
await invoke('sentinel_ratify_prior', { proposalId: proposal_id })
```

### Step 6 — Verify clients pick it up

```js
const active = await invoke('sentinel_get_active_paste_classifier')
console.log(active)   // expect { version: 'paste-vNEXT', eval_tpr, eval_fpr, ... }
```

Restart the app (or wait for the next session start). Console should log:

```
[sentinel] paste classifier upgraded to DAO model paste-vNEXT (TPR=… FPR=…)
```

The Sentinel dashboard Profile tab also surfaces the loaded model and
the active DAO model side by side.

---

## Procedure 2: Emergency — kill the classifier

Use when the paste classifier is producing unacceptable false positives
in production and you need clients to fall back immediately.

```js
await invoke('sentinel_set_kill_switch', {
  req: {
    model_kind: 'paste_classifier_weights',
    active: true,
    reason: 'FP rate spike on android; revisit after retrain',
    actor: '<your-stake-addr>',
  },
})
```

Effect: `sentinel_get_active_paste_classifier` returns `None` on every
session start. Clients keep using their bundled `paste-v1.onnx`. To
disable the bundled signal too, users can flip the per-signal toggle in
the Sentinel dashboard (Paste Classifier card) — or you can ship an app
update with the bundled artifact removed.

To restore:

```js
await invoke('sentinel_set_kill_switch', {
  req: { model_kind: 'paste_classifier_weights', active: false },
})
```

Inspect the current state:

```js
await invoke('sentinel_get_kill_switch', { modelKind: 'paste_classifier_weights' })
```

---

## Procedure 3: Roll back a specific version

Use when one ratified version is bad but newer/older versions are fine.
Avoids the global kill switch.

```js
await invoke('sentinel_blocklist_version', {
  req: {
    model_kind: 'paste_classifier_weights',
    version: 'paste-vNEXT',
    reason: 'FP regression vs v1; reverting',
    actor: '<your-stake-addr>',
  },
})
```

Effect: `sentinel_get_active_paste_classifier` filters this version
out of the candidate list. Selection cascades to the next-newest
gate-passing row (e.g. the previous `paste-v1`).

To un-block once the issue is resolved:

```js
await invoke('sentinel_unblocklist_version', {
  modelKind: 'paste_classifier_weights',
  version: 'paste-vNEXT',
})
```

---

## Procedure 4: Diagnose a stuck classifier

Symptom: `sentinel_get_active_paste_classifier` returns `None` despite a
ratified row existing.

Checklist (in order):

1. **Kill switch active?**
   ```sql
   SELECT * FROM sentinel_kill_switch WHERE model_kind='paste_classifier_weights' AND active=1;
   ```
2. **Version blocklisted?**
   ```sql
   SELECT * FROM sentinel_weights_blocklist WHERE model_kind='paste_classifier_weights';
   ```
3. **Gate fails?**
   ```sql
   SELECT version, eval_tpr, eval_fpr
   FROM sentinel_priors
   WHERE model_kind='paste_classifier_weights'
   ORDER BY ratified_at DESC;
   ```
   Need `eval_tpr >= 0.92` AND `eval_fpr <= 0.03`.
4. **Envelope unreachable?** Check the Rust log for
   `weights candidate ... failed re-verify: envelope resolve timed out`
   or `envelope resolve failed`. Resolve the Iroh peer or re-pin the
   envelope CID locally.
5. **DB/envelope mismatch?** Log message
   `weights_cid mismatch (db=..., envelope=...)` means a local DB
   tamper. Re-fetch from a trusted peer and reset.

---

## Procedure 5: Bump the synthetic generator

When you legitimately change a generator distribution (e.g. add a new
attack class), the golden hashes will fail. To bump cleanly:

1. Edit `cli/src/synth/generators.rs` distributions.
2. Bump `SYNTH_VERSION` in `cli/src/synth/blob.rs` (currently `"v2"`) to the next version (e.g. `"v3"`).
3. Update the `golden_hashes_match_synth_v2` test in `generators.rs`:
   - Rename it to match the new version (e.g. `golden_hashes_match_synth_v3`).
   - Update the version assertion (e.g. `SYNTH_VERSION == "v3"`).
   - Regenerate hashes using the recipe in the test doc-comment.
4. Run `cargo test -p alex synth` — must pass.
5. Retrain the classifier (Procedure 1), because the training corpus
   distribution has shifted.

---

## Threat-model checklist

Before any classifier ratification, confirm:

- [ ] Envelope's `eval_cid` references a Python `eval.py`-generated JSON
      where `macro_tpr` and `macro_fpr` match the envelope's claimed
      values within `1e-6`. (Backend re-checks this; don't rely on it
      alone.)
- [ ] Training script `train.py` was run against the published prior
      corpus, not a private superset.
- [ ] `tools/sentinel-train/eval.py --out eval.json` was run with
      `--threshold 0.5`. Lower thresholds invalidate the gate
      assumptions.
- [ ] No face-related features in the training pipeline. (Per
      [sentinel-federation.md](sentinel-federation.md) decision 2 — face
      is never federated.)
- [ ] Holdout was generated with `synth-sentinel generate-holdout`
      (seed-base 100000+) and not reused as training data.

---

## Incident-response template

When the production classifier behaves badly, file an incident note
with these fields:

```
Date:             <ISO 8601>
Severity:         P1 / P2 / P3
Symptom:          (e.g. FPR spike in iOS users after paste-v2 rollout)
Affected version: paste-v?
Action taken:     (kill switch / blocklist / app rollback)
Root cause:       (TBD on first save)
Followups:        (retrain / threshold tune / new attack class / etc)
```

The DAO's Sentinel committee owns the postmortem. Until threshold-sig
infrastructure lands ([sentinel-federation.md](sentinel-federation.md) §12),
the kill-switch + blocklist IPCs are the operator's authoritative
overrides — they trump any ratified row in `sentinel_priors`.
