# Interview signal feed and suggested follow-ups

Triage: ready-for-agent
Repos: alexandria
Part of: docs/sentinel-trust-profiles-spec.md
Blocked by: docs/sentinel-trust-profiles/03-response-process-signals.md, docs/sentinel-trust-profiles/07-probe-contract.md

Spec: Probes → interview bullet; User stories 22, 23, 25.

- Live feed to the interviewer in Alexandria-hosted interviews: relay latency vs the candidate's own baseline, gaze-down before answer, second-voice presence via voice activity detection (licence-clean model, no transcription, no storage).
- Suggested follow-ups drawn from criteria-authored interview follow-ups.
- Interviews held by video call, phone or in person carry no Sentinel assurance unless a paired attested phone was present.
- Microphone use is consent-gated per session.

## Notes

2026-11-12, deviations and choices made while implementing:

- Voice activity detection is pure DSP, not a model: frame energy against an adaptive noise floor, plus normalised autocorrelation for voicing and pitch (`sentinel::voice_activity`). There are no weights, so there is no weight licence and no sha256 lockfile or CI checksum step. Silero VAD (MIT code and weights) was considered and not used. It only separates speech from non-speech, and the signal needed here is a second voice, which the pitch tracker gives in the same pass. It would also be the only model inference under the `core` profile, which the spec keeps inference-free.
- "Second voice" means a voice whose median pitch is more than 4.5 semitones from the first voice heard on the device (learned from its first 1.5 s of voiced speech). A second speaker with a similar pitch is missed, and a whisper has no pitch. The signal stays advisory (`voice_then_response`, `Info`) until the corpus measures it.
- Voice checks listen to the call's own microphone stream (a tap in front of the Opus encoder), so the microphone is never opened twice. They stop while the call is muted, and they ignore the microphone while the far end is audible and for 300 ms after, because desktop calls have no echo cancelling. The backend turns them on only for a running interview-purpose session whose `consent_microphone` is recorded. The candidate's waiting room has a separate voice-checks choice, nothing preselected, and that choice is the session's microphone consent.
- The feed runs only on the candidate's device, in the candidate integrity check. The panel's device sends a question marker over the call's encrypted room channel (`ALXIS1`). The candidate's app treats the question as asked once the far end has gone quiet for 400 ms, then takes the candidate's next onset in their own voice as the answer. It sends back the latency, the median of the candidate's first three answers, and whether a gaze-down or a second voice came within 3 s before the answer. A second voice is sent as soon as it is heard. An answer that starts before the question ends is not timed.
- Spoken answers are a new response input kind, `voice`, with no reading floor, because answering a heard question at once is ordinary turn-taking. Relay lag is judged against the candidate's own spoken turns. Interview sessions neither read nor write the device's assessment response history.
- Follow-ups authored per criterion: migration 37 adds `interview_criteria.follow_ups_json`. An uncovered criterion suggests its next authored follow-up ahead of the generic one. When the candidate's answer comes with a concern (lag of 3 s or more over their baseline, gaze-down before, or second voice before), the panel gets a suggestion for the criterion the question was about: the next authored follow-up, or a generic step-by-step one if none is left. Follow-ups are written in the preparation view, which covers panel seats too, since Cloud criteria carry none.
- Bullet 3: `build_integrity_report` now accepts an interview held by video call, phone or in person only when the session reached `device_attested` (ticket 12's paired phone, or a mobile device's own attestation), and refuses it otherwise. Interviews held outside Alexandria still have no UI that starts a check, and Cloud's handling of such reports is out of scope.
- Review fix: a signal's `sender` came from the payload, so any node in the room, or a relay, could speak as a panelist. Signals are now signed by the sending node's key over the room topic (prefix `ALXIS2`), the sender is the signing node, and on the candidate's side question markers and live segments are accepted only from the panel: the bootstrap peers of the room ticket Cloud handed over. A second panelist who joined through the first's ticket is not in that set; listing panel nodes would need Cloud's room response to carry them. `hiring_interview_live_segment` no longer takes `marked_by`: it records only a segment the current call received from the panel (`CallManager::panel_live_segment`).
- Not done at the time (since done in ticket 09): no verification result is recorded for interviews. The panel's judgement lives on the panelist's device, and the `live` rung needs it to reach the candidate's session. The voice-check consent copy is English-only for now (listed as pending in the i18n parity check).
