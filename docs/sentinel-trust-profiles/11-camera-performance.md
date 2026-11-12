# Camera pipeline performance

Triage: ready-for-agent
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
