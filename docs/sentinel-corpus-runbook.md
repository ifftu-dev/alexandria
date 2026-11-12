# Sentinel Measurement Corpus Runbook

Status: live
Updated: 2026-11-12

How maintainers build the Sentinel measurement corpus, evaluate it and certify signals. A signal affects a session's outcome only once a reviewed certification admits it with a measured rate; until then it is advisory. Background: [sentinel-trust-profiles-spec.md](sentinel-trust-profiles-spec.md) (Measurement corpus and evaluation); how certified ratios feed the fusion and the coverage statements: [sentinel.md](sentinel.md) §Fusion and session outcome and §Measurement and certification. Retraining the bundled models is [sentinel-runbook.md](sentinel-runbook.md).

## Pieces

| Piece | Where |
|---|---|
| Formats, evaluator, certification check | `crates/alexandria-studio/src/integrity/measurement/` |
| Evaluator binary | `cargo run -p alexandria-studio --example integrity_evaluate -- help` |
| Synthetic fixture corpus + contract certification | `crates/alexandria-studio/fixtures/integrity-corpus/` |
| Real corpora (red-team and consented clean) | `crates/alexandria-studio/corpus/<corpus-id>/` |
| Opt-in session capture | `src-tauri/src/sentinel/corpus_capture.rs` |

A corpus is a `manifest.json` listing entry files by `blake3:` digest, plus a corpus digest over that list. An entry is one session: its committed windows (raw `WindowFeatures`, or flags the backend recorded) with the ground-truth `label` (`clean` or the attack name), `provenance`, trust `profile`, `platform` and `device_class`. The evaluator re-derives every flag from the raw features with the current scoring code, so an entry stays useful after thresholds change.

Synthetic entries are for contract tests only. A certification built on a corpus with any synthetic entry is marked `synthetic` and can never score a session.

## Consent and what is captured

- **Red-team sessions** are run by maintainers on their own devices, following a script below.
- **Clean sessions** come only from participants who opt in explicitly, at the start of a dedicated session, by signing the capture consent. It is separate from the app's terms and from any assessment they take for real. Ordinary sessions are never captured: capture is off unless the app process is started with the capture variable set.
- Captured per window: the aggregate features the backend already receives (counts, means, the latest probe readings, model outputs) and the committed snapshot row. Not captured: keystrokes, pointer paths, camera frames, audio, answers.
- The consent says the entry will be committed to the public repository. A participant can withdraw: delete the entry, reseal the manifest, and re-run any certification that rested on it.

## Capturing a session

1. Start the app with capture on. Desktop: `ALEXANDRIA_SENTINEL_CAPTURE_DIR=<dir> <app binary>`. iOS: pass the same variable through `xcrun devicectl device process launch --environment-variables`. Android has no capture path yet.
2. Run the attempt (scripted attack or clean). Each committed window is appended to `<dir>/<session_id>.jsonl`.
3. Build the entry and add it:

   ```sh
   cargo run -q -p alexandria-studio --example integrity_evaluate -- entry \
     --capture <dir>/<session_id>.jsonl --id <entry-id> --label <clean|attack> \
     --provenance <red_team|consented_clean> --profile <core|attested|camera|full> \
     --platform <macos|windows|linux|ios|android> --device-class <class> \
     > crates/alexandria-studio/corpus/<corpus-id>/entries/<entry-id>.json
   ```

   Append `{ "path": "entries/<entry-id>.json" }` to the manifest's `entries`, then run `integrity_evaluate seal <manifest>`. The digest diff shows up in review.

`entry` refuses a red-team session labelled `clean` and a consented session labelled as an attack.

## Red-team scripts

Run each script for a full attempt (at least ten windows) on every platform and device class the profile supports, at least five sessions each. Perform the attack at least twice per session, at varied points. Label the entry with the script name.

| Script | Profiles | What the maintainer does |
|---|---|---|
| `paste_answer` | all | Copies answers from another document and pastes them in |
| `answer_lookup` | all | Switches to a browser or notes app to look answers up |
| `cheat_overlay` | all (desktop) | Runs an interview-cheat overlay tool during the attempt |
| `remote_helper` | all (desktop) | A second person answers over a remote-desktop session |
| `phantom_hotkeys` | all (desktop) | Drives an overlay tool with its global hotkeys |
| `split_screen` | all (mobile) | Reads answers from a second app in split view or slide-over |
| `screen_mirror` | all | Mirrors or extends the screen to a display a helper watches |
| `accessibility_reader` | all (Android) | Runs a third-party accessibility service that reads the screen |
| `in_app_assistant` | all | Opens the in-app assistant during the attempt |
| `external_assistant` | all (desktop) | Asks an external assistant connected over MCP |
| `bot_input` | all (desktop) | Types and moves the pointer with an automation script |
| `phone_glance` | camera, full | Reads answers from a phone held below the screen |
| `second_person` | camera, full | A second person sits in view and dictates |
| `photo_spoof` | camera, full | Holds a printed photo or a replayed video to the camera |
| `virtual_camera` | camera, full | Feeds the camera from a virtual-camera driver |

Some attacks are invisible by design (a second person out of view, a second phone out of frame). Script them too: an attack no signal catches is a coverage statement fact, not a gap to hide.

**Clean protocol.** The participant takes an ordinary attempt at their own pace with their usual habits: looking at the keyboard, pausing to think, reading aloud, an occasional notification. Capture at least as many clean sessions per profile and platform as attack sessions.

## Evaluating and certifying

```sh
E="cargo run -q -p alexandria-studio --example integrity_evaluate --"
$E report <manifest>                    # per-signal precision, recall, fpr, llr; fusion confusion matrix
$E draft <manifest> --version <v> --date <yyyy-mm-dd> --threshold core=<llr> ...
$E check <manifest> <certification>     # exit 1 if a certified rate is not met
```

Real-world holdouts (committee-held, encrypted with the `sentinel_holdout_*` commands) are evaluated with the same `report`/`check` commands after the committee decrypts them, outside CI.

To certify:

1. Run `draft` with an operating threshold per profile. It lists every signal seen in attacks with its measured rates (floored to four decimals) and per-window likelihood ratio.
2. Review the draft. Remove signals measured on too few sessions or with a false-positive rate the profile cannot carry. A signal left out stays advisory.
3. Commit it as `crates/alexandria-studio/corpus/<corpus-id>/certification-<version>.json` and add a `check` line for it to the CI step "Sentinel certification gate".
4. Point `sentinel::scoring::active_manifest()` at the certification's `session_manifest()`, which refuses a synthetic certification. Coverage statements embed the certification's digest, the `blake3:` hash of its exact file bytes, which `check` prints.

Changing a threshold, the signal set or the scoring code behind an admitted signal means a new certification. CI replays every committed certification on every backend change and fails the build when an admitted signal drops below its certified precision or recall.
