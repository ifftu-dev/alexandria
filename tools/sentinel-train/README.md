# Sentinel Paste-Classifier Training Kit

Trains the ONNX model loaded by `src-tauri/src/sentinel/paste_classifier.rs`
via `tract-onnx` (embedded into the Rust backend at compile time via
`include_bytes!` from `src-tauri/resources/sentinel/paste-v1.onnx`).

**Out-of-tree by intent.** Per `docs/sentinel-federation.md`, the training
side-channel does not ship with the Alexandria client. This directory exists
inside the monorepo for convenience while the pipeline is being bootstrapped;
once Phase 2b ratifies the first model, move this to its own repo so the
client build never pulls Python deps.

## Pipeline

```
synth-sentinel  ->  featurize.py  ->  train.py  ->  paste-v1.onnx
   (Rust)            (numpy)         (torch)        (bundled in app)
        |
        v
   eval.py  ->  TPR / FPR report  ->  sentinel_holdout_evaluate
```

1. Generate data:
   ```bash
   cargo run -p alex -- synth-sentinel generate-all --out-dir ./priors
   cargo run -p alex -- synth-sentinel generate-holdout --out-dir ./holdout
   ```

2. Featurize blobs into `(X, y)` arrays:
   ```bash
   python3 featurize.py --in ./priors --out ./train.npz
   python3 featurize.py --in ./holdout --out ./holdout.npz
   ```

3. Train MLP + export ONNX:
   ```bash
   python3 train.py --train ./train.npz --out ./paste-v1.onnx
   ```

4. Eval against holdout:
   ```bash
   python3 eval.py --model ./paste-v1.onnx --holdout ./holdout.npz --out eval.json
   ```

5. Ship: copy `paste-v1.onnx` to `src/assets/models/paste-v1.onnx` (and bundle
   via `tauri.conf.json` `resources` once added). The DAO-ratified path
   (Phase 4) replaces this manual copy.

## Why MLP, not transformer

The plan referenced a small transformer encoder over digraph sequences. The
shipped Sentinel feature path aggregates a snapshot into a 12-dim vector
*before* inference (see `src-tauri/src/sentinel/features.rs` and the
matching `featurize.py` in this directory — both bit-identical), so a
2-layer MLP is the right model. The transformer option stays open if a
later sequence-level feature path lands.

## Gate thresholds

A model only ratifies (Phase 4) if its `eval.json` reports:

- macro TPR ≥ 0.92
- macro FPR ≤ 0.03
- `paste_macro` TPR ≥ 0.98
- `llm_paste_edit` TPR ≥ 0.85
