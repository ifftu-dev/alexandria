# Sentinel — Assessment Integrity System

> Anti-cheat system for Alexandria that monitors learning-session integrity through multi-signal behavioral fingerprinting. No biometric or sensitive data ever leaves the device — only derived scores (0-1) and anomaly flags are stored locally.

## Design Principles

1. **Privacy-first** — All behavioral data (keystrokes, mouse movements, video frames) is processed entirely on-device. Only numeric scores and categorical flags are stored in the local database.
2. **Non-punitive by default** — Sentinel informs rather than punishes. Flagged sessions surface for review; automated suspensions require multiple strong signals.
3. **Dual scoring** — Rule-based and AI-based systems run in parallel. Rule-based is authoritative today; AI is advisory until validated with labeled data.
4. **On-device ML only — backend-resident.** All ML runs in the Rust backend. The paste classifier uses `tract` (pure-Rust ONNX inference) with weights embedded at compile time via `include_bytes!` or hot-swapped from a DAO-ratified CID. The per-user keystroke autoencoder and mouse-trajectory CNN train + score via `candle` (Apache-2.0, HuggingFace) inside the same crate. The face embedder remains pure-pixel LBP math — no ML framework involved. The frontend only buffers raw events and forwards them to the backend over Tauri IPC.
5. **Incremental trust** — Behavioral profiles build over time. New users start with generous defaults; consistency scoring activates after 10+ samples.

## Architecture Overview

```
┌── Frontend (Tauri WebView, Vue 3) ─────────────────────────────────┐
│                                                                    │
│  Event buffers:                                                    │
│    keystroke[]  ──┐                                                │
│    mouse[]      ──┼─► useSentinel() composable                     │
│    canvas pixels┘    - rule analyzers (variance, paste size, etc.) │
│                      - LBP face embedder (pixel math, local)       │
│                      - IPC dispatch on snapshot                    │
└─────────────────────────────────────┬──────────────────────────────┘
                                      │ Tauri IPC
                                      ▼
┌── Backend (Rust crate `app_lib`, src-tauri/) ──────────────────────┐
│                                                                    │
│   commands/sentinel_ml ◄─── all ML IPC entry points                │
│        │                                                           │
│        ├─► sentinel::paste_classifier (tract, ONNX inference)      │
│        │     - bundled paste-v1.onnx via include_bytes!            │
│        │     - hot-swap to DAO-ratified weights at session start   │
│        │                                                           │
│        ├─► sentinel::keystroke_ae (candle, autograd)               │
│        │     - per-user 4→8→4→8→4 autoencoder, contrastive train   │
│        │                                                           │
│        ├─► sentinel::mouse_cnn (candle dense + hand-rolled conv)   │
│        │     - reservoir conv + trainable 160→32→1 head            │
│        │                                                           │
│        └─► sentinel::features (12-dim windowed feature extractor)  │
│                                                                    │
│   commands/integrity ◄── integrity_sessions + integrity_snapshots  │
│   commands/sentinel_priors ◄── DAO model distribution + safety     │
│        valves (kill switch, version blocklist, three-layer verify) │
│                                                                    │
│   SQLite (sqlcipher):                                              │
│     sentinel_user_models      — per-user AE + CNN weights (JSON)   │
│     sentinel_priors           — labeled attack data + DAO weights  │
│     sentinel_kill_switch      — operator override                  │
│     sentinel_weights_blocklist— per-version rollback set           │
└────────────────────────────────────────────────────────────────────┘
```

All processing happens client-side. There is no server-side component — Alexandria is a fully decentralized desktop and mobile application. Integrity scores are stored locally in SQLite; the current implementation does not publish Sentinel snapshots over P2P.

## Signal Taxonomy

| Signal | Source | Type | Weight | Description |
|--------|--------|------|--------|-------------|
| `typing_consistency` | Rule | 0-1 | 0.20 | EMA-based deviation from stored typing profile |
| `mouse_consistency` | Rule | 0-1 | 0.15 | Velocity deviation from stored mouse profile |
| `is_human_likely` | Rule | bool | 0.15 | Velocity variance check (bots have constant speed) |
| `tab_switches` | Rule | count | 0.15 | Tab focus changes during assessment |
| `paste_events` / `pasted_chars` | Rule | count | 0.10 | Clipboard paste activity |
| `devtools_detected` | Rule | bool | 0.10 | DevTools heuristic |
| `face_present` / `face_count` | Rule | bool/int | 0.15* | Face verification every 3s while camera opted in (* opt-in only; loop driven by the course player) |
| `ai_keystroke_anomaly` | AI | 0-1 | 0.05† | Autoencoder reconstruction error |
| `ai_mouse_human_prob` | AI | 0-1 | 0.05† | CNN human vs bot classification |
| `ai_face_similarity` | AI | 0-1 | 0.05† | LBP histogram cosine similarity (every 3s) |
| `ai_face_match` | AI | bool | advisory | Whether face matches enrollment (drives `face_mismatch` flag when false) |
| `ai_paste_anomaly` | AI | 0-1 | 0.05† | ONNX classifier — probability snapshot is paste / typing-bot / LLM-paste-edit. Drives `paste_classifier_anomaly` (≥0.95) and `paste_classifier_critical` (≥0.99) flags. |
| `gaze_offscreen_ratio` | AI | 0-1 | advisory | Fraction of camera ticks the learner's gaze was estimated off-screen (second-device / look-away detection). Drives `gaze_wander` (>0.30), `device_glance` (≥3 downward glances), `gaze_occluded` (>0.40 occluded). See §Gaze. |

† Only applied when advisory AI scoring is toggled on (see §Runtime Toggle).

## Rule-Based System (Active)

### Client-Side (`useSentinel.ts`)

- **Activation**: Starts when an enrolled learner opens the course player; the active element context is updated as they navigate
- **Snapshot interval**: Random 15-45 seconds
- **Profile storage**: localStorage keyed by `sentinel_profile_{userId}_{deviceFp[0:16]}`
- **Profile update**: Exponential Moving Average with alpha=0.2 (alpha=0.5 during training wizard)
- **Consistency scoring**: Activates after `sampleCount > 10`
- **Integrity score**: Weighted average of all signals (see weights above)

### Flagging Logic

Per-snapshot checks:
1. Low integrity score (< 0.40) → `low_integrity` warning
2. Low consistency score (< 0.35) → `behavior_shift` warning
3. Excessive tab switching (> 10) → `tab_switching` info
4. Excessive pasting (> 500 chars) → `paste_detected` warning
5. DevTools detected → `devtools_detected` critical
6. Bot-like mouse variance → `bot_suspected` critical
7. Face absent (camera opted in) → `no_face` info
8. Multiple faces → `multiple_faces` warning
9. Face identity mismatch (enrolled) → `face_mismatch` critical
10. Prolonged absence (5+ consecutive checks ~15s) → `prolonged_absence` warning
11. Frequent absence (>50% of checks in snapshot window) → `frequent_absence` info
12. Paste classifier ≥ 0.95 → `paste_classifier_anomaly` warning
13. Paste classifier ≥ 0.99 → `paste_classifier_critical` critical
14. Gaze off-screen ratio > 0.30 (camera opted in) → `gaze_wander` warning
15. ≥ 3 downward off-screen glances in the window → `device_glance` critical
16. Gaze-occluded ratio > 0.40 (eyes hidden while camera opted in) → `gaze_occluded` warning

Flag severity is authoritative on the backend (`commands/integrity.rs::flag_severity`). Unknown flags default to info so client/server version skew never auto-suspends a session.

Session outcome determination:
- **Clean**: Default
- **Flagged**: 1 critical OR 3+ warnings OR integrity < 0.40
- **Suspended**: 2+ critical OR (1 critical + 2 warnings)

**Where it runs:** the Rust backend re-evaluates status on every `integrity_submit_snapshot` and once more at `integrity_end_session`. Severity counters are denormalized on `integrity_sessions.critical_count` / `warning_count` for O(1) evaluation. Per-snapshot `anomaly_flags` are persisted as JSON on `integrity_snapshots.anomaly_flags` (migration 036). Status only promotes in severity mid-session (`active → flagged → suspended`); a clean session finalizes as `completed` at end.

### Trust Factor Integration

The trust signal is the session itself. When a session ends `flagged` or `suspended`, the backend records the terminal `status` + `integrity_score` on the `integrity_sessions` row; that is what downstream credential issuance reads. For observability it also computes a spec-pinned penalty (logged, not persisted to a row):
- Critical violation: −0.20 per flag
- Warning: −0.10 per flag
- Info: no penalty

> **Post-VC-first cutover (migration 040):** the legacy per-`evidence_records` `trust_factor` decay — and the SkillProof aggregator that read it — were retired along with the `evidence_records` / `skill_proofs` / `skill_assessments` tables. There is no per-evidence trust column to decay anymore; integrity feeds the credential decision via the session's status/score rather than by mutating an evidence row.

## AI Models (Advisory)

### 1. Keystroke Autoencoder

**Location**: `src-tauri/src/sentinel/keystroke_ae.rs`

**Architecture**: 4→8→4→8→4 autoencoder with ReLU activations. Real autograd via [candle](https://github.com/huggingface/candle); training runs in the Rust backend with `candle_nn::optim::SGD`.

**Input features** (per digraph pair): `[dwellMs1, dwellMs2, flightMs, speedRatio]` — normalized using per-user mean/std computed during training.

**Training**: On-device from calibration wizard data or accumulated assessment data. Requires 20+ digraph samples. 80 epochs, learning rate 0.005.

**Inference**: Reconstruction error averaged across all digraphs in the snapshot window. Mapped to [0,1] via sigmoid calibrated against training loss (5× `trainLoss` ≈ 0.5 anomaly score). A 0.05 floor is applied to `trainLoss` to prevent ratio blow-up when users train with very consistent typing.

**Anomaly threshold**: 0.65 — above this, the typing pattern doesn't match the enrolled user.

**Storage**: weights persist as a JSON blob in `sentinel_user_models` (composite PK `user_address, device_fp_prefix, model_kind='keystroke_ae'`). Backing store is the already-sqlcipher-encrypted SQLite DB — replaces the legacy `localStorage` location. Never broadcast over P2P.

### 2. Mouse Trajectory CNN

**Location**: `src-tauri/src/sentinel/mouse_cnn.rs`

**Architecture**: Conv1D(3→8, k=5) → ReLU → MaxPool(2) → Conv1D(8→16, k=3) → ReLU → MaxPool(2) → Dense(160→32) → ReLU → Dense(32→1) → Sigmoid

**Input**: 50-point trajectory segments with 3 channels (dx, dy, dt), normalized per-segment.

**Training**: Dense layers trained via backprop. Conv layers act as random feature extractors (reservoir computing approach — deliberate trade-off for on-device training speed). Negative examples are 5 synthetic bot patterns: constant velocity, linear interpolation, sine wave, jittered straight line, instant teleport.

**Inference**: Average prediction across all 50-point segments in the buffer. Output is probability of human input.

**Human threshold**: 0.50

**Storage**: weights persist as a JSON blob in `sentinel_user_models` (`model_kind='mouse_cnn'`). Conv kernels are not stored — they are deterministic given the seed, recomputed on load. Only the dense-head parameters (~5 KB) round-trip through the DB.

### 3. Face Embedder (LBP)

**Location**: `src/utils/sentinel/face-embedder.ts`

**Algorithm**: Local Binary Pattern histograms with 4x4 spatial binning over detected face region. 59 uniform LBP bins per grid cell → 944-dimensional embedding, L2-normalized.

**Face detection**: YCbCr skin-color segmentation with bounding box extraction.

**Enrollment**: Running average of embeddings over 5+ frames during calibration wizard.

**Verification**: Cosine similarity between enrollment embedding and live frame embedding.

**Match threshold**: 0.70

**Storage**: ~8KB (944 floats) in `localStorage` under `sentinel_profile_{userId}_{deviceFp[0:16]}` → `aiModels.faceEnrollment`. Never persisted to SQLite or broadcast over P2P.

**Advantage over skin-ratio**: Can distinguish between different people, not just "face present vs absent". Robust to lighting changes (LBP is contrast-invariant).

### 4. Paste / Typing-Bot Classifier (ONNX)

**Location**: `src-tauri/src/sentinel/paste_classifier.rs` (tract inference), `src-tauri/src/sentinel/features.rs` (12-dim feature extractor)

**Architecture**: MLP 12 → 32 → 16 → 1 (sigmoid). Trained offline on synthetic adversarial data (see [sentinel-adversarial-priors.md](sentinel-adversarial-priors.md) §Phase 2). Per-sample normalization mean/std is **baked into the first linear layer** so the on-device forward pass takes raw 12-dim features without preprocessing.

**Input features** (windowed over the snapshot's keystroke buffer):

| # | Feature |
|---|---------|
| 0 | mean dwell ms |
| 1 | std dwell ms |
| 2 | mean flight ms |
| 3 | std flight ms |
| 4 | fraction of digraphs with flightMs < 5 (paste-burst rate) |
| 5 | max consecutive near-zero-flight run length, normalized to /200 |
| 6 | char rate (chars/sec), normalized to /50 |
| 7 | dwell coefficient of variation (std/mean) |
| 8 | flight coefficient of variation |
| 9 | paste event count, capped + normalized to /10 |
| 10 | pasted character count, capped + normalized to /1000 |
| 11 | keystroke buffer length, normalized to /200 |

The Rust extractor in `sentinel::features` and the Python featurizer in `tools/sentinel-train/featurize.py` are **bit-identical**. If you change one, change the other in the same commit or the trained model will silently mispredict. There is no third copy on the frontend any more — the JS side only buffers raw events and forwards them via IPC.

**Inference runtime**: `tract-onnx` 0.21 in the Rust backend. Pure Rust, no WASM, no CDN. The ONNX bytes are embedded at compile time via `include_bytes!("../../resources/sentinel/paste-v1.onnx")` so there is no filesystem race or asset-protocol handshake. DAO-supplied weights from `sentinel_load_dao_classifier` go through the same tract parse/optimize/runnable path.

**Platform support**: every Tauri target the Rust crate compiles for — macOS, Linux, Windows, iOS, Android. The previous WKWebView / Android-WebView gate is **gone** because nothing about ML runs in the WebView anymore.

**Anomaly thresholds**:
- 0.95: emits `paste_classifier_anomaly` (severity: warning)
- 0.99: emits `paste_classifier_critical` (severity: critical) — combined with one other warning, triggers session suspension per the existing flag-severity rule

**Model artifact**: `src-tauri/resources/sentinel/paste-v1.onnx` (~4.6 KB, weights inline; PyTorch `dynamo=False` export so tract can parse without sidecar `.data` files). Pinned via `src-tauri/resources/sentinel/paste-v1.onnx.sha256` lockfile (CI verifies on every push). The DAO-update path (see §Runtime Model Updates) can replace the active session at runtime without an app upgrade.

**Storage**: No per-user state. The classifier is a global attack detector — calibration is done via the existing `KeystrokeAutoencoder` for per-user personalization. The two scores blend `0.7 * onnx + 0.3 * autoencoder` once both are available.

**Training**: `tools/sentinel-train/{featurize,train,eval}.py`. Outputs ONNX opset 17. Default 30 epochs, AdamW lr=3e-4, batch=128, label smoothing 0.05, 2× class weight on the `llm_paste_edit` class.

**Holdout gate**: A trained model only ratifies if `macro_tpr >= 0.92`, `macro_fpr <= 0.03`, `paste_macro` TPR ≥ 0.98, and `llm_paste_edit` TPR ≥ 0.85. The synthetic-only v1 release achieves macro TPR=1.0 / macro FPR=0.0; expect lower numbers when real-world holdout data joins (Phase 5).

**Bytes caps**: Defense in depth against malicious envelopes pointing at huge CIDs:
- Envelope + eval JSON: 1 MiB max (`MAX_WEIGHTS_BLOB_BYTES`, Rust side)
- ONNX weights: 50 MiB max (`MAX_WEIGHTS_BYTES` in `sentinel_priors.rs`, also enforced in `sentinel_ml::sentinel_load_dao_classifier`)
- Resolver round trips: 5 s timeout (`WEIGHTS_RESOLVE_TIMEOUT`) per CID fetch

### 5. Gaze / Second-Device Detector

**Location**: `src-tauri/src/sentinel/face_detect.rs` (YuNet detection via tract), `src-tauri/src/sentinel/gaze.rs` (head-pose + calibration), `src-tauri/src/commands/sentinel_gaze.rs` (IPC).

**Purpose**: catch the second-device cheat class — glancing down at a phone or sideways at a second monitor during an assessment. Honest scope: it flags off-screen *gaze*, so it catches look-away behaviour but not an earpiece + reader, nor a device propped directly beside the webcam. A strong add, not a gate on its own.

**Pipeline** (all backend, frames never leave the device):

1. **Face detection** — YuNet (`face_detection_yunet_2023mar`, OpenCV Zoo, **MIT**) embedded via `include_bytes!`, run through `tract`. The model is baked at a fixed **640×640** input (its declared input fact; other sizes fail tract shape inference), so the frame is letterboxed in and detections are inverted back to original coordinates. Anchor-free 3-stride decode (8/16/32) yields bbox + **5 landmarks** (right/left eye, nose, right/left mouth) + score, then NMS. Output contract (12 tensors) is asserted in `model_contract_holds`.
2. **Head-pose proxies** — derived geometrically from the 5 landmarks (dep-free, no solvePnP): `yaw` = horizontal nose offset from the eye midpoint over inter-ocular distance; `pitch` = nose position between the eye line and mouth line; `roll` = eye-line angle. Plus a coarse `iris` offset (darkness-weighted centroid in an eye box).
3. **Gaze estimate** — two paths:
   - **Uncalibrated**: threshold the raw yaw/pitch proxies against a generous on-screen cone. No enrollment needed; coarse look-away detection.
   - **Calibrated**: a per-user `5 → 16 → 2` MLP (candle SGD) maps `[yaw, pitch, roll, iris_dx, iris_dy]` to a normalized screen point; off-screen = predicted point outside the unit square + margin. Trained from the wizard's 9-point capture.

**Calibration is license-clean by construction**: it trains on the user's *own* 9-point look-at-the-dot capture, so no external gaze dataset (e.g. Gaze360 / MPIIGaze, which forbid commercial models-trained-on-dataset) is ever involved.

**Storage**: calibration weights persist as a JSON blob in `sentinel_user_models` (`model_kind='gaze_calib'`), same convention as the keystroke AE / mouse CNN. Never broadcast over P2P. Reset by `sentinel_reset_user_models`.

**Flags** (computed per snapshot window in `useSentinel`, severity authoritative in `commands/integrity.rs::flag_severity`):
- `gaze_wander` (warning) — off-screen ratio > 0.30
- `device_glance` (critical) — ≥ 3 downward off-screen glances in the window (the phone-in-lap tell)
- `gaze_occluded` (warning) — eyes hidden (no usable face geometry) for > 0.40 of ticks

The per-window `gaze_offscreen_ratio` is persisted on `integrity_snapshots` (migration 060) for dashboard observability.

**Frontend**: `Player.vue`'s camera loop calls `sentinel.scoreGaze(video)` (fire-and-forget) alongside the existing LBP presence check. It downscales the frame to ≤320px and forwards RGBA over IPC; cadence is intended to be assurance-level dependent (faster in high-assurance mode). The 9-point calibration is a step in `SentinelTrainingWizard.vue`.

**Phase 2 (deferred)**: precise point-of-regard via a MediaPipe iris model (Apache-2.0, needs a tflite→ONNX conversion); a gaze × keystroke-timing correlation signal (the high-confidence "copying" flag); face *recognition* via ArcFace (runtime-trivial on tract, but weights are license-gated — buy an InsightFace commercial license). See [sentinel-gaze.md](sentinel-gaze.md) for the full design.

### Runtime Toggle

AI signals are advisory by default and do not contribute to the integrity score. A per-device toggle (`sentinel_ai_scoring_enabled` in localStorage, exposed in the Sentinel dashboard Profile tab) folds them in at a 0.05 weight each:
- `(1 − ai_keystroke_anomaly)` × 0.05
- `ai_mouse_human_prob` × 0.05
- `ai_face_similarity` × 0.05 (only if camera opted in)
- `(1 − ai_paste_anomaly)` × 0.05 (only if both the master AI toggle AND the per-signal `sentinel_paste_classifier_enabled` toggle are on)

Total advisory contribution is capped at 0.20 (all four signals at once), well under any single rule-based weight. Toggle off if false-positive rate spikes.

**Per-signal opt-out**: the paste classifier has its own toggle in the Sentinel dashboard ("Paste Classifier (ONNX)") backed by `sentinel_paste_classifier_enabled` in localStorage. Defaults to `on`; flipping it off keeps the other AI signals contributing. Useful if the paste classifier specifically generates FPs.

## Runtime Model Updates

The paste classifier supports two model sources at runtime:

1. **Bundled** — `src-tauri/resources/sentinel/paste-v1.onnx`, embedded into the Rust binary at compile time via `include_bytes!`. Always available; loaded once at process start by the `sentinel::paste_classifier` `OnceLock`.
2. **DAO-ratified** — A `paste_classifier_weights` row in `sentinel_priors`, signed by the Sentinel DAO. Discovered at session start via `sentinel_get_active_paste_classifier`; bytes fetched via `content_resolve_bytes(weights_cid)` and swapped into the active `InferenceSession`.

### Selection + verification

The IPC returns the **newest gate-passing** weights row that survives a three-layer content-addressed re-verification:

1. **Operator overrides** — short-circuit before any DB scan:
   - If `sentinel_kill_switch` row for `paste_classifier_weights` is `active=1`, return `None`.
   - `sentinel_weights_blocklist` rows filter the candidate set by `(model_kind, version)`.
2. **DB filter** — `eval_tpr >= 0.92 AND eval_fpr <= 0.03 AND model_kind = 'paste_classifier_weights' AND weights_cid IS NOT NULL`. Ordered by `(ratified_at DESC, version DESC)`.
3. **Layer 1 ↔ 2 (envelope re-verify)** — re-fetch the envelope blob at `cid` (5 s timeout, 1 MiB cap), parse, confirm `WeightsBlobMeta` matches DB columns within `1e-6` epsilon, and the envelope-reported gate still passes. Defense against a locally tampered DB.
4. **Layer 2 ↔ 3 (eval re-verify)** — re-fetch the eval JSON at `meta.eval_cid` (same timeout + size cap), parse, confirm `macro_tpr` / `macro_fpr` match the envelope's claims and pass the gate. Defense against a DAO-published envelope with cooked claimed metrics.
5. **First survivor wins.** If nothing passes, the IPC returns `None` and the client uses the bundled artifact.

Failure modes — all fall back to bundled, never crash monitoring:
- Kill switch active → `None`, log a warning naming the model_kind
- Resolver unavailable (early boot, no peers) → return the gate-only top candidate; client side validates the bytes (size cap still applies on `loadFromDaoBytes`)
- Envelope missing / timeout / parse error → skip, try next
- DB column / envelope `weights_cid` mismatch → skip, try next
- Eval JSON missing / mismatch → skip, try next
- Bytes don't form a valid ONNX session → `loadFromDaoBytes` returns false, bundled stays active

### Signature

`signature` on a weights row is currently the `compute_prior_signature` Blake2b digest over `(cid|label|model_kind|schema_version)`. **This is not an authenticated signature — anyone can compute it.** It binds metadata to the row but doesn't certify DAO ratification. Until the real threshold-sig infrastructure lands ([sentinel-federation.md](sentinel-federation.md) §12), the safeguards are:

- `sentinel_ai_scoring_enabled` defaults to `false` on every device.
- Kill switch (`sentinel_set_kill_switch`) globally disables the classifier without an app update.
- Version blocklist (`sentinel_blocklist_version`) rolls back a single faulty version.
- Server-side re-verify in `verify_weights_candidate` re-fetches envelope + eval JSON, rejects mismatches.

See [sentinel-runbook.md](sentinel-runbook.md) for operator procedures.

## Operator IPCs

| Command | Description |
|---------|-------------|
| `sentinel_set_kill_switch` | Toggle kill switch by `model_kind`. Active=true forces `sentinel_get_active_paste_classifier` to return `null`. |
| `sentinel_get_kill_switch` | Read current kill-switch state. |
| `sentinel_blocklist_version` | Block a specific `(model_kind, version)` from selection. Idempotent. |
| `sentinel_unblocklist_version` | Remove a block. |

## Database Schema

```sql
integrity_sessions       -- One per learning session
  └── integrity_snapshots  -- Random-interval measurements, includes ai_paste_anomaly REAL (migration 044)
                         --   and gaze_offscreen_ratio REAL (migration 060)
sentinel_user_models     -- Per-user candle weights: keystroke_ae, mouse_cnn, gaze_calib (JSON blobs)
sentinel_priors          -- DAO-ratified attack patterns AND classifier weights
                         --   keystroke / mouse: labeled-samples blobs
                         --   paste_classifier_weights: model bundle (migration 045 adds
                         --   weights_cid, eval_cid, eval_tpr, eval_fpr, version columns)
sentinel_kill_switch     -- Operator-controlled disable per model_kind (migration 046)
sentinel_weights_blocklist -- (model_kind, version) pairs the selector must skip (migration 046)
```

Stored in local SQLite. See [Database Schema](database-schema.md) for full DDL.

## IPC Commands

| Command | Description |
|---------|-------------|
| `integrity_start_session` | Start integrity monitoring for an enrolled learning session |
| `integrity_get_session` | Get session with scores |
| `integrity_end_session` | End session and compute final score |
| `integrity_submit_snapshot` | Submit a behavioral snapshot (includes `ai_paste_anomaly`) |
| `integrity_list_sessions` | List all sessions |
| `integrity_list_snapshots` | List snapshots for a session |
| `sentinel_propose_prior` | Propose a labeled-samples or weights blob to the Sentinel DAO |
| `sentinel_ratify_prior` | Finalize an approved proposal into `sentinel_priors` |
| `sentinel_priors_list` | List ratified priors (optionally filtered by `model_kind`) |
| `sentinel_priors_load` | Fetch + re-validate a prior's blob |
| `sentinel_priors_sync` | Pull newly-ratified priors from peers |
| `sentinel_get_active_paste_classifier` | Return the newest gate-passing, re-verified weights row, or null |
| `sentinel_score_paste` | Extract 12-dim features + score via tract. Single round-trip per snapshot. |
| `sentinel_paste_classifier_info` | Loaded model source + version (`bundled` / `dao`) |
| `sentinel_load_dao_classifier` | Replace the active tract session with DAO-supplied ONNX bytes |
| `sentinel_revert_classifier_to_bundled` | Drop the DAO session, fall back to embedded weights |
| `sentinel_train_keystroke_ae` | Train (or fine-tune) the per-user keystroke autoencoder via candle |
| `sentinel_score_keystroke_ae` | Score keystrokes against the user's AE; `-1.0` if not yet trained |
| `sentinel_extract_digraphs` | Pull `DigraphFeatures` from raw keystroke events |
| `sentinel_train_mouse_cnn` | Train the per-user mouse-trajectory CNN dense head via candle |
| `sentinel_score_mouse_cnn` | Score mouse points; `-1.0` if not yet trained |
| `sentinel_user_models_status` | List per-user model rows (epochs, samples, loss, updated_at) |
| `sentinel_reset_user_models` | Wipe per-user AE + CNN + gaze-calib weights for `(user, device)` |
| `sentinel_detect_face` | Run YuNet on a frame; return bbox + 5 landmarks + score per face |
| `sentinel_extract_gaze_features` | Head-pose + iris features for the best face (wizard calibration capture) |
| `sentinel_score_gaze` | Detect → load per-user calibration → return `GazeEstimate` + face count |
| `sentinel_train_gaze_calib` | Fit/refit the per-user gaze calibration MLP from 9-point samples |

## UI

### Sentinel Dashboard (`/dashboard/sentinel`)

4-tab interface:
- **Overview**: Live engine status, no-profile prompt, session statistics
- **Sessions**: Paginated session list with outcome badges, expandable details
- **Signals**: Signal weight breakdown, threshold documentation
- **Profile**: Stored behavioral profile details, AI model training status

### Training Wizard

6-step calibration flow (similar to Face ID setup):
1. **Welcome** — Explains what Sentinel does and the privacy guarantee
2. **Typing Calibration** — User types a reference paragraph; system captures digraph timing
3. **Mouse Calibration** — Click-target game with 8 randomly placed targets
4. **Awareness** — Explains what signals are monitored during assessments
5. **Camera** (optional) — Face detection test + LBP enrollment
6. **Review** — Summary of calibrated profile + AI model training results

### Learning Page Integration

`useSentinel` lifecycle hooks in the course player:
- `start()` on mount (if enrolled)
- `setElement()` on element navigation
- `stop()` on unmount
- `setCameraOptedIn(bool)` when the learner accepts/declines camera verification on an assessment element
- A `setInterval(3000)` in `Player.vue` calls `sentinel.verifyFace(videoEl)` while the camera stream is live; the hidden `<video>` element is torn down on disable or unmount

## Privacy Guarantees

These guarantees are architectural — they are enforced by the code structure, not by policy.

1. **Raw keystrokes never stored**: Only anonymized timing features (dwell/flight in ms). The `key` field is set to `'char'` for all printable characters.
2. **Raw mouse coordinates never transmitted**: Only deltas (dx, dy, dt) used for CNN features; absolute positions stay in the short-lived buffer.
3. **Video frames never leave the device**: Face processing happens on a `<canvas>` element. Frames are forwarded to the Rust backend over in-process Tauri IPC for YuNet detection + gaze estimation, processed in memory, and **never persisted** — only derived values (944-float embedding, skin ratio, gaze yaw/pitch, off-screen ratio) are stored. The gaze calibration model encodes a statistical pose→screen mapping, not recoverable imagery.
4. **AI model weights are not biometric data**: Autoencoder/CNN weights encode statistical patterns of typing/movement, not recoverable input data. LBP embeddings cannot be reverse-engineered into face images. Published *adversarial priors* (labeled cheat patterns and DAO-ratified classifier weights, curated by the Sentinel DAO — see [sentinel-adversarial-priors.md](sentinel-adversarial-priors.md)) contain no individual user data; they are catalog content, not per-user telemetry.
5. **Profile keyed to device**: `sentinel_profile_{userId}_{deviceFingerprint[0:16]}` — profiles are device-specific.
6. **No server-side data**: All behavioral processing happens on-device. The Rust backend stores only numeric scores and categorical flags in local SQLite. The Sentinel DAO-published prior/weights library is read-only from each client's perspective and carries no user identifiers — clients consume it, they never produce to it unless the learner explicitly proposes a pattern.
7. **Inference is local**: The paste classifier runs entirely in the Rust backend via `tract` (pure Rust); the ONNX bytes are embedded at compile time with `include_bytes!`, so there is no runtime fetch from a CDN and no remote inference path. (The earlier ONNX Runtime Web / WASM backend was retired — see "Inference runtime" above.)
8. **DAO weights are bounded**: Incoming weights blobs are capped at 1 MiB (envelope/eval JSON) and 50 MiB (ONNX bytes); resolver round trips time out at 5 s. A malicious envelope cannot trigger unbounded download or memory allocation.
