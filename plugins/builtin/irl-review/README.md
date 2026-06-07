# IRL Review

Lets a learner upload work in any format (image, audio, video, PDF,
plain document) and ask a human instructor for a review. Instructors
see pending submissions in **Settings → Plugins → IRL Review → Instructor
Inbox** and reply with:

- An overall score (0–100%)
- Freeform written feedback
- A rating per skill the learner self-declared

The learner's submission view updates with the reply once it lands.

## How it works

1. **Learner submits.** The plugin shows file picker + skill tags input
   + comment field. On **Submit**, the files are base64-encoded
   client-side and passed to the host via `alex.submit(...)` with
   `metadata.type = 'irl_review'`. The host stores a row in
   `plugin_irl_submissions` with status `pending`.

2. **Instructor reviews.** From Settings → Plugins → IRL Review, the
   instructor sees every pending submission across this node. Opening
   one shows the file previews, the declared skills, and a form to post
   a review.

3. **Learner sees the result.** Back inside the plugin (or in the
   "My Submissions" tab of the Settings page), the row shows the score,
   feedback, and per-skill ratings once the instructor has replied.

## Configuration

The host passes element-level config via the `content` payload:

```json
{
  "default_skills": ["composition", "color-theory"],
  "prompt": "Upload your final portfolio piece and tell us what you were aiming for.",
  "accept": "image/*,application/pdf"
}
```

All fields are optional. `accept` falls back to `*/*` (any file type).

## Capabilities

No special capabilities required. Files are loaded into memory inside
the iframe and forwarded through the host postMessage channel — they
never touch `navigator.mediaDevices` or `fetch()`.

## Privacy

Submissions stay on this node. There is no network round-trip; the
instructor must be using the same device (or share access to the same
profile). Network-mediated review routing is a later phase.
