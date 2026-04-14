# Sentinel — Assessment Integrity System

> Anti-cheat system for Alexandria that monitors learning-session integrity through multi-signal behavioral fingerprinting. No biometric or sensitive data ever leaves the device — only derived scores (0-1) and anomaly flags are stored locally.

## Design Principles

1. **Privacy-first** — All behavioral data (keystrokes, mouse movements, video frames) is processed entirely on-device. Only numeric scores and categorical flags are stored in the local database.
2. **Non-punitive by default** — Sentinel informs rather than punishes. Flagged sessions surface for review; automated suspensions require multiple strong signals.
3. **Dual scoring** — Rule-based and AI-based systems run in parallel. Rule-based is authoritative today; AI is advisory until validated with labeled data.
4. **Zero dependencies for AI** — All ML models are hand-written in TypeScript. No external ML frameworks, WASM runtimes, or model downloads.
5. **Incremental trust** — Behavioral profiles build over time. New users start with generous defaults; consistency scoring activates after 10+ samples.

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────────┐
│                     CLIENT (Tauri WebView)                       │
│                                                                 │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────────────┐  │
│  │  Keystroke    │  │  Mouse       │  │  Face                │  │
│  │  Autoencoder  │  │  Trajectory  │  │  Embedder            │  │
│  │  (4→8→4→8→4) │  │  CNN (1D)    │  │  (LBP Histogram)     │  │
│  └──────┬───────┘  └──────┬───────┘  └──────────┬───────────┘  │
│         │                 │                      │              │
│  ┌──────▼─────────────────▼──────────────────────▼───────────┐  │
│  │              useSentinel() Composable                      │  │
│  │  Rule-based analyzers + AI model scoring + EMA profiling   │  │
│  └──────────────────────────┬────────────────────────────────┘  │
│                             │ scores + flags only               │
│                             ▼                                   │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │  Tauri IPC → Rust Backend                                │   │
│  │  integrity_sessions + integrity_snapshots (SQLite)       │   │
│  └──────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────┘
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
| `face_present` / `face_count` | Rule | bool/int | 0.15* | Continuous face verification every 3s (* camera opt-in only) |
| `ai_keystroke_anomaly` | AI | 0-1 | advisory | Autoencoder reconstruction error |
| `ai_mouse_human_prob` | AI | 0-1 | advisory | CNN human vs bot classification |
| `ai_face_similarity` | AI | 0-1 | advisory | LBP histogram cosine similarity (continuous, every 3s) |
| `ai_face_match` | AI | bool | advisory | Whether face matches enrollment (continuous, every 3s) |

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
5. DevTools detected → `tab_switching` critical
6. Face absent (camera opted in) → `no_face` info
7. Multiple faces → `multiple_faces` warning
8. Face identity mismatch (enrolled) → `face_mismatch` warning
9. Prolonged absence (5+ consecutive checks ~15s) → `prolonged_absence` warning
10. Frequent absence (>50% of checks in snapshot window) → `frequent_absence` info

Session outcome determination:
- **Clean**: Default
- **Flagged**: 1 critical OR 3+ warnings OR integrity < 0.40
- **Suspended**: 2+ critical OR (1 critical + 2 warnings)

### Trust Factor Integration

Confirmed violations lower `trust_factor` on `skill_assessments` by 0.20 per violation (floor: 0.10). This propagates through the evidence pipeline — flagged assessment evidence carries less weight in skill proof aggregation and instructor reputation attribution.

## AI Models (Advisory)

### 1. Keystroke Autoencoder

**Location**: `src/composables/sentinel/keystroke-autoencoder.ts`

**Architecture**: 4→8→4→8→4 autoencoder with ReLU activations, trained via SGD with full backpropagation.

**Input features** (per digraph pair): `[dwellMs1, dwellMs2, flightMs, speedRatio]` — normalized using per-user mean/std computed during training.

**Training**: On-device from calibration wizard data or accumulated assessment data. Requires 20+ digraph samples. 80 epochs, learning rate 0.005.

**Inference**: Reconstruction error averaged across all digraphs in the snapshot window. Mapped to [0,1] via sigmoid calibrated against training loss (5x training loss = 0.5 anomaly score).

**Anomaly threshold**: 0.65 — above this, the typing pattern doesn't match the enrolled user.

**Storage**: ~8KB JSON weights in `BehavioralProfile.aiModels.keystrokeAutoencoder`.

### 2. Mouse Trajectory CNN

**Location**: `src/composables/sentinel/mouse-trajectory-cnn.ts`

**Architecture**: Conv1D(3→8, k=5) → ReLU → MaxPool(2) → Conv1D(8→16, k=3) → ReLU → MaxPool(2) → Dense(160→32) → ReLU → Dense(32→1) → Sigmoid

**Input**: 50-point trajectory segments with 3 channels (dx, dy, dt), normalized per-segment.

**Training**: Dense layers trained via backprop. Conv layers act as random feature extractors (reservoir computing approach — deliberate trade-off for on-device training speed). Negative examples are 5 synthetic bot patterns: constant velocity, linear interpolation, sine wave, jittered straight line, instant teleport.

**Inference**: Average prediction across all 50-point segments in the buffer. Output is probability of human input.

**Human threshold**: 0.50

**Storage**: ~200KB JSON weights in `BehavioralProfile.aiModels.mouseCNN`.

### 3. Face Embedder (LBP)

**Location**: `src/composables/sentinel/face-embedder.ts`

**Algorithm**: Local Binary Pattern histograms with 4x4 spatial binning over detected face region. 59 uniform LBP bins per grid cell → 944-dimensional embedding, L2-normalized.

**Face detection**: YCbCr skin-color segmentation with bounding box extraction.

**Enrollment**: Running average of embeddings over 5+ frames during calibration wizard.

**Verification**: Cosine similarity between enrollment embedding and live frame embedding.

**Match threshold**: 0.70

**Storage**: ~8KB (944 floats) in `BehavioralProfile.aiModels.faceEnrollment`.

**Advantage over skin-ratio**: Can distinguish between different people, not just "face present vs absent". Robust to lighting changes (LBP is contrast-invariant).

## Database Schema

```sql
integrity_sessions       -- One per learning session
  └── integrity_snapshots  -- Random-interval measurements
```

Stored in local SQLite. See [Database Schema](database-schema.md) for full DDL.

## IPC Commands

| Command | Description |
|---------|-------------|
| `integrity_start_session` | Start integrity monitoring for an enrolled learning session |
| `integrity_get_session` | Get session with scores |
| `integrity_end_session` | End session and compute final score |
| `integrity_submit_snapshot` | Submit a behavioral snapshot |
| `integrity_list_sessions` | List all sessions |
| `integrity_list_snapshots` | List snapshots for a session |

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
- Camera opt-in component rendered when camera is enabled

## Privacy Guarantees

These guarantees are architectural — they are enforced by the code structure, not by policy.

1. **Raw keystrokes never stored**: Only anonymized timing features (dwell/flight in ms). The `key` field is set to `'char'` for all printable characters.
2. **Raw mouse coordinates never transmitted**: Only deltas (dx, dy, dt) used for CNN features; absolute positions stay in the short-lived buffer.
3. **Video frames never leave the device**: Face processing happens on a `<canvas>` element. Only the derived embedding (944 floats) or skin ratio (single float) is stored.
4. **AI model weights are not biometric data**: Autoencoder/CNN weights encode statistical patterns of typing/movement, not recoverable input data. LBP embeddings cannot be reverse-engineered into face images.
5. **Profile keyed to device**: `sentinel_profile_{userId}_{deviceFingerprint[0:16]}` — profiles are device-specific.
6. **No server-side data**: All behavioral processing happens on-device. The Rust backend stores only numeric scores and categorical flags in local SQLite.
