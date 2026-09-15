# Interview Assistant

> Instructor-facing, local-first support for planning, conducting, transcribing,
> and reviewing structured interviews. The attributed transcript is the durable
> record; raw audio and video are not recorded in the current release.

## Scope and status

The interview assistant is available in Instructor mode at `/interviews`. It
reuses Alexandria's live tutoring transport and visual language while keeping
the interview record in a separate, private data model.

The shipped workflow includes:

- standalone interview plans or plans linked to a sponsored role assessment;
- an objective, duration, ordered criteria, and interviewer/candidate/observer
  participants;
- explicit, per-participant choices for transcription, Sentinel, and camera
  processing before a session starts;
- live audio/video, screen sharing, invitations, chat, device controls, and
  connection diagnostics from live tutoring;
- speaker-attributed local or remote captions, with manual attributed entry as
  the universal fallback;
- deterministic follow-up suggestions based on the latest answer and uncovered
  criteria;
- coverage tracking, private interviewer notes, and an editable summary and
  conclusion; and
- a JSON export that uses participant pseudonyms and excludes private notes by
  default.

This is deliberately not a raw-media recorder. The `record_audio` and
`record_video` schema fields reserve the consent/data-model boundary for future
work, but the current UI keeps both off and stores no audio or video recording.
The follow-up recommender and summary generator are deterministic local helpers,
not remote or generative-AI services.

## User flow

1. **Prepare** — create a plan, optionally select a role assessment, name the
   participants, and define the objective and criteria.
2. **Record choices** — review each participant separately. A decline is a
   valid recorded choice; the interview only requires that everyone has made a
   choice, not that every optional capability is accepted.
3. **Conduct** — create the encrypted live room, capture attributed text, mark
   criteria, save private notes, and accept or dismiss follow-up suggestions.
4. **Review** — edit the generated evidence-oriented draft, record a conclusion,
   inspect the transcript and criteria coverage, and export if needed.
5. **Delete** — delete immediately or allow the configured retention period to
   expire.

The routes are:

| Route | Purpose |
|---|---|
| `/interviews` | Plan and list interviews |
| `/interviews/:id` | Consent/preflight and live interview workspace |
| `/interviews/:id/review` | Transcript, coverage, summary, conclusion, and export |

## Architecture

```text
participant microphone
        │
        ▼
on-device speech recognition ── final text only ──► encrypted tutoring room
        │                                                │
        │ local final segment                            │ remote final segment
        └──────────────────────────┬─────────────────────┘
                                   ▼
                     consent + participant match
                                   │
                                   ▼
                    per-profile SQLCipher database
                  transcript / criteria / notes / review
```

Live media and room messages remain owned by `tutoring`. The interview command
module owns only the durable record. This separation lets tutoring captions be
useful without silently turning every tutoring room into a recorded interview.

### Speech-to-text and speaker attribution

`useLocalSpeechRecognition` exposes speech recognition only when the WebView
supports the explicit `processLocally` control. If that guarantee is absent,
speech recognition is treated as unavailable; Alexandria does not fall back to
a hosted transcription service. Manual attributed transcript entry remains
available.

A participant who enables captions sends final text, confidence when available,
their iroh node id, and room display name over the existing encrypted room
gossip channel. The internal payload is prefixed with `ALXTR1` so it cannot be
decoded as an older chat message. Tutoring keeps these messages in memory only.

The interview host persists a remote segment only after matching it to an
interview participant who consented to transcription. Matching prefers the
participant's stored `peer_id`, then an exact normalized display name, then the
sole eligible participant. Ambiguous messages remain unassigned instead of
being guessed. The persisted `speaker_label` is taken from the participant row,
not trusted from the incoming message.

### Follow-up recommendations and summaries

The current recommender is deterministic and runs against the local transcript.
It can ask for:

- a baseline or measurement when an outcome is asserted without a number;
- the candidate's individual contribution when an answer uses collective
  language;
- failure modes and mitigations when an answer discusses a system component;
- a concrete, step-by-step example after a short answer; or
- evidence for the next criterion still marked `not_covered`.

At most three suggestions are added for a trigger, and each can be marked
`asked` or `dismissed`. The summary generator produces an editable draft from
criterion coverage and the five latest timestamped transcript highlights. It
does not infer a hiring decision.

### Sentinel

Interview sessions can use the complete Sentinel signal pipeline already
available to assessments. Monitoring runs on the conductor device and therefore
describes activity visible to that device; it is not remote surveillance of a
candidate on another device. The interviewer attached to the conductor device
is monitored first, with a candidate-only session as the fallback.

Sentinel starts only when that participant has accepted Sentinel processing.
Camera-derived checks additionally require their camera choice. An integrity
session is stored with `purpose = 'interview'`. During the live interview,
warnings and critical findings can mark the integrity session `flagged`, but do
not suspend or end the interview assistant workflow. Interview-purpose sessions
also do not stage camera frames for the assessment appeal-evidence flow.

## PII and privacy boundaries

| Boundary | Current behavior |
|---|---|
| Storage | Interview rows live only in the active profile's SQLCipher database. |
| Sync and global gossip | Interview tables are excluded from cross-device sync and libp2p gossip. |
| Room transport | Final caption text and a room display name travel only to current peers over the encrypted tutoring room. Relays can still observe connection metadata. |
| Consent | Choices are recorded per participant and capability. A participant with transcription disabled or revoked cannot have a segment appended. |
| Audio/video | Used for the live call, but no raw interview recording is retained. |
| Speech recognition | Enabled only with an explicit on-device-processing capability; there is no cloud fallback. |
| Attribution | Durable segments require a known, consenting participant. Ambiguous remote speakers are not persisted. |
| Notes | Interviewer notes are local and private; they are excluded from export unless the instructor deliberately includes them. |
| Export | Pseudonyms are on by default. Including names or private notes is an explicit local export choice. |
| Retention | Configurable from 1–365 days, default 30. Expired interviews are hidden and purged when the interview list opens; they can also be purged or deleted explicitly. |

Export downloads a JSON file outside Alexandria's encrypted per-profile storage
boundary. The instructor is responsible for the download destination and
subsequent handling of that file.

## Data model and IPC

Migration 092 adds six interview tables and adds `purpose` to
`integrity_sessions`:

- `interview_sessions`
- `interview_participants`
- `interview_criteria`
- `interview_transcript_segments`
- `interview_notes`
- `interview_followups`

The `commands/interview.rs` module exposes create/list/get, consent, transcript,
follow-up, criterion, note, lifecycle, summary/review, deletion, and expiry-purge
operations. See [Database Schema](database-schema.md) for the table summary and
[Sentinel](sentinel.md) for integrity semantics.

## Shared live-session UI

The interview and tutoring screens share `LiveParticipantTile`,
`LiveSessionControls`, and `LiveDiagnosticsModal`. The tutoring screen also has
participant-controlled captions using the same on-device capability gate and
encrypted transcript envelope. It does not persist those captions. Existing
debug support remains available: connection/peer state, ticket and QR invite,
chat, mic and output levels, media toggles, iOS audio-device selection, and the
raw diagnostics JSON view.
