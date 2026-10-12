# Interview Assistant

> Local-first support for planning, conducting, transcribing, and reviewing
> structured interviews — for instructors, and for panelists on an
> organisation's interview held in Alexandria. The attributed transcript is the
> durable record; raw audio and video are not recorded in the current release.

## Scope and status

The interview assistant is available in Instructor mode at `/interviews`. It
reuses Alexandria's live tutoring transport and visual language while keeping
the interview record in a separate, private data model. The same session and
review pages also serve as a panel seat on an interview an organisation
schedules in Alexandria Cloud (see [Panel seats](#panel-seats-on-an-organisations-interview));
those live under `/panel` and do not need Instructor mode.

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
  default; and
- for a panel seat, a scorecard sent to the organisation from the review.

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
| `/panel/:id` | A panel seat: the organisation's brief, consent/preflight and the live workspace |
| `/panel/:id/review` | The same review, plus the scorecard for the organisation |
| `/interview-room/:id` | The candidate's way into an interview held in Alexandria |

## Panel seats on an organisation's interview

An organisation can hold an interview inside Alexandria instead of on a meeting
link. It schedules the interview in Alexandria Cloud with **Where: In
Alexandria**, the criteria the panel assesses, and one to six panelists. Cloud
returns each panelist a join link once:

```text
alexandria://interview/join?cloud=<Cloud origin>&id=<interview>&panelist=<seat>&token=<token>
```

The token is the seat's only credential, so the link is accepted only on the
custom scheme (never an `https` URL that browsers and servers log), and its
Cloud origin must be `https`, or `http` on loopback for local testing.

**Opening the link.** The desktop app (as a deep link, or pasted under
Settings → Organisations → *On an interview panel?*) hands the link to
`interview_panel_open` (`commands/interview_panel.rs`). It fetches the
interview from Cloud with `Authorization: Panelist <token>` and creates an
ordinary local interview session: the organisation's criteria, the panelist as
interviewer, the other panelists as interviewers, and the candidate. Opening
the same link again reuses that session. The page shows the organisation, role
and candidate, the chosen time, the panel, whether the room is open, and any
skills the candidate has shared with that organisation.

**Starting.** Consent works as for any interview: every requested
participant's choices are recorded first. On **Start interview**, the app asks
Cloud whether the room is open. If it is, the panelist joins it. If not, the
panelist creates the tutoring room and posts its ticket to Cloud
(`interview_panel_set_room`), so the first panelist in opens the room and
everyone else joins it.

**The candidate.** The candidate accepts the invitation under **Settings →
Organisations**. An accepted interview held in Alexandria then appears under
*Upcoming interviews*, and **Join interview** opens 15 minutes before the
chosen time. `/interview-room/:id` asks Cloud for the ticket with a request
signed by the learner's own key (`hiring_interview_room`), so only the
candidate the interview is for can learn it. Until a panelist has opened the
room, the page checks again every few seconds. Joining enters the ordinary
live tutoring room. The candidate's app keeps no interview record beyond the
answer they signed and, if they ran the integrity check, the result they sent. The
candidate's captions follow the tutoring rules: made only if the candidate
turns them on, and kept in memory only.

**Scorecard.** From the review, a panelist can send the organisation a
scorecard (`interview_panel_scorecard`): for each criterion, a rating from 1
to 4 or none and the session's coverage status, plus a recommendation (strong
yes, yes, no, strong no) and notes of up to 4,000 characters. Cloud shows the
scorecards side by side on its interview page.

**Sentinel.** A panel seat never starts Sentinel. Integrity monitoring belongs
on the candidate's device, and the seat's session is created with Sentinel
off. The organisation can ask for it there: an invitation with
`integrity_check` makes the waiting room ask the candidate, before **Join**,
whether to run the check and whether to allow camera checks. If they run it,
the summary their app sends when they leave the call (outcome, score, flag
counts, camera coverage) appears on the panel seat, on the scorecard page and
in Cloud, marked as the candidate's app's own report. See
[Sentinel](sentinel.md#candidate-integrity-check).

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
| Candidate integrity check | Runs only if the organisation asked and the candidate agreed. Only the summary leaves the candidate's device; the candidate keeps a copy of what they sent. |
| Panel seats | The transcript, private notes, summary and conclusion stay on the panelist's device. Only the scorecard the panelist sends — ratings, coverage per criterion, a recommendation and its notes — goes to the organisation's Cloud. The seat's token is stored with the local session so the review can still reach Cloud. |
| Export | Pseudonyms are on by default. Including names or private notes is an explicit local export choice. |
| Retention | Configurable from 1–365 days, default 30. Expired interviews are hidden and purged when the interview list opens; they can also be purged or deleted explicitly. |

Export downloads a JSON file outside Alexandria's encrypted per-profile storage
boundary. The instructor is responsible for the download destination and
subsequent handling of that file.

## Data model and IPC

The baseline schema holds six interview tables and the `purpose` column on
`integrity_sessions`:

- `interview_sessions`
- `interview_participants`
- `interview_criteria`
- `interview_transcript_segments`
- `interview_notes`
- `interview_followups`

Migration 16 (`interview_panel_link`) adds `cloud_url`, `cloud_interview_id`,
`cloud_panelist_id`, `cloud_panel_token` and `cloud_org` to
`interview_sessions`, with one session per Cloud interview and seat.
Migration 17 (`interview_integrity_reports`) keeps the candidate's copy of each
integrity result they sent.

The `commands/interview.rs` module exposes create/list/get, consent, transcript,
follow-up, criterion, note, lifecycle, summary/review, deletion, and expiry-purge
operations. `commands/interview_panel.rs` adds `interview_panel_open`,
`interview_panel_status`, `interview_panel_set_room` and
`interview_panel_scorecard`; the candidate's `hiring_interview_room`,
`hiring_interview_integrity_send` and `hiring_interview_integrity_sent` are in
`commands/hiring.rs`. See [Database Schema](database-schema.md) for the table summary and
[Sentinel](sentinel.md) for integrity semantics.

## Shared live-session UI

The interview and tutoring screens share `LiveParticipantTile`,
`LiveSessionControls`, and `LiveDiagnosticsModal`. The tutoring screen also has
participant-controlled captions using the same on-device capability gate and
encrypted transcript envelope. It does not persist those captions. Existing
debug support remains available: connection/peer state, ticket and QR invite,
chat, mic and output levels, media toggles, iOS audio-device selection, and the
raw diagnostics JSON view.
