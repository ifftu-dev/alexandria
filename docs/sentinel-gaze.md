# Sentinel Phase 1 — Gaze / Second-Device Detection (Design Spec)

Status: **Phase 1 landed.** Scope: MVP gaze + head-pose "looking away from screen" detector to catch the
second-device cheat class. All-OSS, commercial-license-clean, runs on `tract` (already in-tree).
For the user-facing summary see [sentinel.md](sentinel.md) §Gaze; this doc is the implementation design
+ rationale (license/runtime analysis, phasing). The Phase 2 items at the end remain proposals.

## 0. Goal & non-goals

**Goal:** detect when an enrolled learner repeatedly looks off-screen (down at a phone in the
lap, sideways at a second monitor) during an assessment, and surface it as integrity flags —
without any new license risk and without the MediaPipe tflite→ONNX conversion tax.

**Catches:** gross look-down / look-away glances, periodic device-glance patterns, gaze-occlusion
(hand/sunglasses). **Does not catch:** earpiece + reader (no gaze signal), device propped right
beside the webcam. Honest scope — this is a strong *add*, not a gate on its own.

**Non-goals for P1:** precise point-of-regard (needs MediaPipe iris — Phase 2), face *recognition*
(license-blocked — separate track), liveness/anti-spoof (MiniFASNet — adjacent, easy add later).

## 1. Model choice (settled)

| Role | Model | License | Runtime | Bundling |
|------|-------|---------|---------|----------|
| Face detect + **5 landmarks** | **YuNet** (OpenCV Zoo) | MIT ✅ | tract, fixed-input ONNX | `include_bytes!` (~340 KB) |
| Head pose | solvePnP on 5 landmarks | pure Rust math | n/a | none |
| Gaze refine | iris-centroid + per-user calib MLP | candle (own data) | candle | `sentinel_user_models` |

YuNet's 5 landmarks (R-eye, L-eye, nose tip, R-mouth, L-mouth) are exactly enough for a 5-point
solvePnP head-pose and for cropping the eye regions. Calibration data is the user's own 9-point
capture → **no external gaze dataset → no Gaze360 license problem.** Sidesteps both traps.

Export YuNet ONNX with a **fixed input shape** (e.g. 320×240×3) so tract's optimizer fully engages
(per the runtime feasibility finding — dynamic shapes disable optimization). Pin a
`yunet.onnx.sha256` lockfile like `paste-v1.onnx.sha256`, CI-verified.

## 2. New backend modules

Mirror the established split: a stateless tract detector (like `paste_classifier.rs`) + a per-user
candle model (like `mouse_cnn.rs`).

### 2a. `src-tauri/src/sentinel/face_detect.rs` (tract, global)

Copy the `paste_classifier.rs` shape verbatim:
- `BUNDLED_YUNET: &[u8] = include_bytes!("../../resources/sentinel/yunet.onnx")`
- `static DETECTOR: OnceLock<RwLock<LoadedDetector>>`
- `build_runnable(bytes)` with `.with_input_fact(0, f32::fact([1,3,240,320]).into())` (fixed),
  `.into_optimized().into_runnable()`
- `detect(frame: &GrayOrRgbFrame) -> Result<Vec<FaceDetection>>` — runs YuNet, applies NMS in
  Rust host code (tract has no NMS op; YuNet exports raw boxes+scores), returns bbox + 5 landmarks
  + score per face.
- Optional `set_dao_session`/`revert_to_bundled`/`loaded_info` later — reuse the DAO CID pipeline
  for a better detector without an app update. Not required for P1; bundled-only is fine to start.

NMS + decode: port YuNet's priors/stride decode (3 strides, anchor boxes) into a small pure-Rust
`decode.rs` helper. ~80 lines. Deterministic, unit-testable against a known frame.

### 2b. `src-tauri/src/sentinel/gaze.rs` (candle, per-user)

Mirror `mouse_cnn.rs`:
- `HeadPose { yaw, pitch, roll }` from `solve_pnp(landmarks5, frame_w, frame_h)`.
  Generic 3D face model (5 canonical points in mm), EPnP/iterative solve. Pure Rust (small
  linear-algebra; `nalgebra` already in tree? if not, hand-roll the 5-point case). Returns Euler
  angles. Unit-test against synthetic projected points with known rotation.
- Iris feature: crop each eye region from eye landmarks (fixed aspect box), grayscale, find iris
  center via darkest-region centroid / radial-symmetry (no model). Output normalized iris offset
  `(idx, idy)` within the eye box per eye.
- **Calibration model** `GazeCalib`: tiny MLP `5 → 16 → 2` (inputs: yaw, pitch, iris_dx, iris_dy,
  roll; outputs: predicted screen x,y in [0,1]). Trained via candle SGD exactly like the mouse CNN
  dense head. `export_weights()/from_weights()` JSON, `train_loss/trained_epochs/training_samples`.
- `estimate(detection, calib) -> GazeEstimate`:
  - calibrated: predict screen (x,y); `on_screen = x∈[-m,1+m] && y∈[-m,1+m]` (margin m≈0.15).
  - uncalibrated fallback: threshold on raw |yaw|,|pitch| with a generous cone (coarse, still
    flags gross look-away). Return `confidence` low.
  - occluded (no/one eye, low det score) → `GazeEstimate{ occluded: true }`.

### 2c. `src-tauri/src/sentinel/types.rs` (extend)

```rust
pub struct FaceFrame { pub width: u32, pub height: u32, pub rgba: Vec<u8> } // or grayscale
pub struct FaceDetection { pub bbox: [f32;4], pub landmarks5: [[f32;2];5], pub score: f32 }
pub struct GazeEstimate {
    pub yaw: f32, pub pitch: f32,
    pub screen_x: Option<f32>, pub screen_y: Option<f32>,
    pub on_screen: bool, pub occluded: bool, pub confidence: f32,
}
```

## 3. IPC surface — `src-tauri/src/commands/sentinel_gaze.rs`

Mirror `sentinel_ml.rs` exactly (async `#[tauri::command]`, `State<AppState>`, reuse the generic
`load_user_model`/`save_user_model` helpers — promote them to a shared module or duplicate).

| Command | Sig | Notes |
|---------|-----|-------|
| `sentinel_detect_face` | `FaceFrame -> Vec<FaceDetection>` | YuNet + NMS. Replaces JS LBP detect. |
| `sentinel_score_gaze` | `{frame, user_address, device_fp_prefix} -> GazeEstimate` | detect → pose → iris → calib. `-1`/`occluded` when no model, mirrors AE/CNN contract. |
| `sentinel_train_gaze_calib` | `{user_address, device_fp_prefix, samples:[{yaw,pitch,iris_dx,iris_dy,roll, target_x,target_y}]}` -> `{train_loss,...}` | from 9-point wizard. `model_kind='gaze_calib'`. |
| `sentinel_gaze_calib_status` | reuse `sentinel_user_models_status` | already lists by model_kind. |

`model_kind = 'gaze_calib'` slots into the existing `sentinel_user_models` composite-PK table —
**no schema change for the calib model.** `sentinel_reset_user_models` already wipes it.

## 4. Integrity wiring — `src-tauri/src/commands/integrity.rs`

### 4a. New flags in `flag_severity` (line ~83)
```rust
"device_glance"   => Severity::Critical,  // repeated downward glance pattern
"gaze_wander"     => Severity::Warning,   // off-screen ratio over threshold
"gaze_occluded"   => Severity::Warning,   // eyes hidden during assessment
```
Unknown-flag-defaults-to-info invariant already protects version skew — keep it.

### 4b. Snapshot signal
Add `gaze_offscreen_ratio REAL` column to `integrity_snapshots` via a new migration (mirror
**migration 044** that added `ai_paste_anomaly REAL`). Extend `IntegritySnapshot` /
`SubmitSnapshotRequest` structs (lines 25/42) + the INSERT (line ~211) + the SELECT (line ~447).
Keeps the persisted-signal pattern consistent and dashboard-readable.

### 4c. Flag emission (frontend, see §5) computes:
- `gaze_offscreen_ratio` = off-screen estimates / total estimates in the snapshot window.
- `gaze_wander` when ratio > 0.30.
- `device_glance` when ≥ N downward (pitch-down + off-screen) glances with periodicity in window.
- `gaze_occluded` when occluded fraction high while camera opted in.

Severity already feeds `critical_count`/`warning_count` denormalization → existing
status-promotion logic (clean→flagged→suspended, lines 107-109) picks it up for free.

## 5. Frontend — `useSentinel.ts` + `Player.vue`

Currently `verifyFace(video)` runs JS LBP (`FaceEmbedder`, `src/utils/sentinel/face-embedder.ts`).
Phase 1 moves detection+gaze to the backend, consistent with principle 4 ("on-device ML only —
backend-resident"):

- In `Player.vue` the existing `setInterval` face loop (line 337) grabs the hidden `<video>`,
  draws to a `<canvas>`, extracts `ImageData`, downscales to 320×240, and `invoke('sentinel_score_gaze', {frame, user_address, device_fp_prefix})`.
  `user_address = stakeAddress`, `device_fp_prefix = deviceFp.substring(0,16)` (same keys already
  used at lines 631/654/673).
- **Cadence = assurance-level dependent:** 1 s in high-assurance mode (catch quick glances), 3 s
  in privacy default (battery). Reuse the assurance flag from the two-mode plan.
- Accumulate `GazeEstimate`s per snapshot window in the composable; in `buildSnapshot` (the IPC
  dispatch block ~line 630-693) compute `gaze_offscreen_ratio` + push the new anomaly flags into
  the `anomaly_flags` array already sent to `integrity_submit_snapshot`.
- LBP `face-embedder.ts` can stay for now (face-present/identity advisory) or be retired once
  YuNet detect + future ArcFace land. Don't delete in P1.

**IPC payload size:** 320×240 RGBA ≈ 300 KB/call; send grayscale (≈76 KB) or JPEG-encode in the
webview first. Tauri IPC is in-process/local — acceptable at 1–3 s cadence. Frames never persisted.

### Wizard calibration step
Add a "Gaze Calibration" step to `SentinelTrainingWizard.vue` (after mouse, before camera/review):
show 9 dots in sequence; at each, capture a few `sentinel_detect_face`→features samples tagged with
the dot's normalized screen coords; on finish call `sentinel_train_gaze_calib`. Reuses the existing
click-target-game UX pattern.

## 6. Privacy (unchanged guarantees)
- Frames processed in the Rust backend, **never stored**; only derived `yaw/pitch/ratio/score`
  persist. Same as existing keystroke/mouse handling. Update `docs/sentinel.md` Privacy section
  (with permission) to cover gaze.
- Calibration model = statistical mapping, not biometric; stored in the already-sqlcipher DB,
  never broadcast over P2P (same as AE/CNN weights).

## 7. Testing (mirror existing module tests)
- `face_detect.rs`: bundled YuNet cold-load + detect on a synthetic/known frame returns ≥1 face;
  NMS dedups overlapping boxes; latency < budget.
- `gaze.rs`: solvePnP recovers known rotation from synthetic projected landmarks (±few°);
  calib MLP training reduces loss + weights roundtrip (copy mouse_cnn tests); off-screen
  classification on synthetic left/right/down gaze; occlusion path returns `occluded`.
- `sentinel_gaze.rs`: untrained calib → fallback; train→score roundtrip via DB.
- Integrity: `flag_severity` returns Critical/Warning for the 3 new flags; unknown still info.

## 8. Migrations / artifacts checklist
- [ ] `src-tauri/resources/sentinel/yunet.onnx` (fixed 320×240 input) + `.sha256` lockfile + CI check
- [ ] migration NNN: `ALTER TABLE integrity_snapshots ADD COLUMN gaze_offscreen_ratio REAL` (mirror 044)
- [ ] register new IPC commands in `src-tauri/src/lib.rs` invoke_handler
- [ ] `nalgebra` (or hand-rolled 5-pt solver) dep check in `Cargo.toml`

## 9. Sequencing inside P1
1. `face_detect.rs` + YuNet bundle + NMS/decode + tests (provable detection foundation).
2. `gaze.rs` head-pose-only path + uncalibrated cone → ships gross look-away detection.
3. IPC + integrity flags + frontend loop swap → end-to-end signal in dashboard.
4. Iris feature + calibration model + wizard step → precision.
5. (Phase 3 hook) gaze×keystroke-timing correlation for the high-confidence "copying" flag.

## 10. Adjacent easy wins (not P1, noted)
- **MiniFASNet liveness** (Apache-2.0, ~2 MB, tract-clean) — same `face_detect.rs` pattern; kills
  the photo/printout impersonator. Low effort once YuNet detect crops faces.
- **ArcFace recognition** — runtime-trivial on tract (int8 ~40 MB) the day the InsightFace
  commercial license clears; distribute via the existing DAO CID pipeline.
