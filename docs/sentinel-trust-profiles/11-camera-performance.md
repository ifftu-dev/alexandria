# Camera pipeline performance

Triage: done
Repos: alexandria
Part of: docs/sentinel-trust-profiles-spec.md

Spec: Implementation Decisions → Performance.

- Binary IPC for frames instead of JSON number arrays; native capture where the existing camera stack allows.
- Detection at the frame's own resolution (re-export or dynamic-shape runtime), slow-cadence detection with landmark tracking between.
- Liveness on trigger (new track, confidence drop), not every tick.
- Inference on its own thread with frame dropping.
- Event-driven overlay and display scans where the platform offers notifications.
- Keep composable edits confined to the camera tick path: ticket 01 is restructuring the same composable concurrently.
- Publish per-profile budgets measured on a reference low-end device class (ticket 05 consumes).

## Notes

### 2026-11-12: implementation

- **Binary IPC.** `sentinel_score_gaze`, `sentinel_extract_gaze_features` and `sentinel_detect_face` take a raw RGBA body (`Uint8Array` over the canvas buffer) with `x-sentinel-frame-width/height`, `x-sentinel-user`, `x-sentinel-device` and `x-sentinel-preview` headers. A JSON body is refused, and `FaceFrame` no longer deserializes.
- **Native resolution.** `yunet.onnx` is re-exported with its baked 640×640 input, output and 94 intermediate shapes dropped. The weights are untouched; the recipe is `scripts/sentinel/export-yunet-dynamic.py`. Every op was already shape-agnostic (the Reshapes use -1 and the Resizes use scales). Frames pad up to a multiple of 32, and one tract plan is cached per padded shape. CI now checks the YuNet and MiniFASNet lockfiles as well as the paste classifier's.
- **Accuracy check** (upstream Silent-Face samples, 224 px longest side, onnxruntime): letterbox-640 against native scored 0.860/0.857 and 0.866/0.828, with boxes within 4 px. Tract at native size scores 0.857 and 0.827.
- **Tracking and triggers.** `sentinel::camera_pipeline` detects every 3 s and patch-tracks the five landmarks between detections. It falls back to detection when tracking quality or face geometry fails. Liveness runs as a burst of 3 reads on a new track, an IoU change, a score drop of 0.15 or more, a tracking loss, or a 30 s refresh, so a snapshot window still gets the 3 reads the advisory spoof rule needs. Between detections the face count carries over, so a second face is seen within one cadence.
- **Inference thread.** `sentinel::inference_worker` holds one pending frame per lane (session ahead of preview). A newer frame replaces a waiting one, and the replaced frame gets `{ "dropped": true }`, which the frontend does not count as a check.

### Measured per-tick cost

`cargo run --release --example sentinel_camera_bench -- <face.jpg>`, Apple M4 Pro, 224×168 frame:

| Stage | Before | After |
|---|---|---|
| IPC payload | 528 KiB JSON, 1.0 ms Rust parse (plus JS encode) | 147 KiB raw, 0.002 ms copy |
| Face detection | 95.5 ms (640² letterbox) | 9.3 ms (224×192) |
| Liveness | 6.8 ms, every tick | 6.8 ms, on trigger only |
| Tick, every tick / mean of 60 ticks at 1 Hz | 102.4 ms | 3.9 ms (20 detected at 9.9 ms, 40 tracked at 1.0 ms, 6 liveness reads) |
| Tick with no face | 102.4 ms | 8.8 ms (every tick detects) |

`camera_budget::benchmark()` on the same machine: detect 8.8 ms, liveness 6.6 ms, track 0.1 ms. A run under heavy load (load average about 65, other builds running) gave detect 44.5 ms, liveness 49.1 ms and track 0.1 ms, which admits `camera` but not `full`. The benchmark measures contention at session start as well as the hardware.

### Budgets (ticket 05)

The budgets are per pass, for a reference low-end device class assumed at about a tenth of an M4 Pro performance core. This assumption is not measured; ticket 16 should confirm it on a device.

- `camera`: detect 250 ms, liveness 60 ms, track 15 ms.
- `full`: detect 150 ms, liveness 40 ms, track 10 ms.

Both detect every 3 s. At 10× the measured cost (about 90 ms detect, 70 ms liveness), a worst-case tick takes about 160 ms against a 1 s tick. The steady state stays under 5% of a core, with headroom for slower parts.

### Deferred

- **Native capture.** The webview's `getUserMedia` stream drives the visible preview, and desktop `nokhwa` is built for enumeration only. Opening the camera natively as well would give it two consumers. iOS calls already hand the webview native JPEG frames. Tapping those in Rust belongs with the feature-gated tutoring video path, not this ticket.
- **Event-driven overlay scan.** The display topology was already event-driven (`sentinel://display`). The hidden-overlay scan runs in the snapshot dispatch that ticket 01 is rewriting. macOS also has no window-creation notification for other processes without per-app Accessibility observers. A Windows `SetWinEventHook` path is possible later.
- **Cadence per profile.** The worker uses the `camera` cadence until ticket 05 passes the chosen profile in (`CameraBudget::pipeline_config`).

- 2026-11-13: closed. Merged with the rest of the spec; review findings fixed alongside.
