# Demo course corpus

Non-authoritative source material. Profile creation, mnemonic restore, and
unlock import the seven full courses as profile-owned drafts using inline
text and quizzes, plus Plugins Showcase and all nine bundled plugins.
Existing course edits are preserved. The seven video tutorials and external
placeholder media are excluded. **Instructor → My courses → Load example
courses** remains available to restore missing example drafts.
Nothing in this source corpus is authoritative: there are no signed rows, identities,
credentials, enrolments, opinions or governance records.

- `courses.json` — 15 example courses and tutorials with their chapters,
  elements, inline lesson content, skill tags and video chapters. It was
  exported from the retired startup seed with its fabricated authorship,
  publication, chain and content-address fields removed. Courses marked
  `provenance: "ai_generated"` (the civics material) are AI-generated example
  content. Plugin elements reference built-in plugin bundles by exact content
  identity (`plugin_cid`).
- `media.json` — the remote video, PDF and text files the retired seed
  downloaded, keyed by element id.

## Licences

The licence notes in `media.json` repeat claims written in the retired seed
code. They are **unverified**. Some are known to be doubtful — for example,
IETF RFC text is published under the IETF Trust's terms rather than placed in
the public domain. Check each source's actual terms, and record the required
attribution, before redistributing or embedding any of these files.

Automatic profile seeding and the **Load plugin showcase** action import `course_plugin_demo`,
resolves its six lessons to the current embedded plugin CIDs/versions, repairs
legacy starter-code newlines, and installs the bundled course plugins.

## Additional local resources

`resources.json` contains seven adapted Opinion discussion examples, three
classroom/channel templates, and two video courses. `videos/` contains their
15 original narrated clips; the JSON preserves every spoken script. These
are AI-generated instructional examples using the macOS Samantha synthetic
voice, not recordings of an instructor. Regenerate with Python + Pillow and
FFmpeg using `scripts/generate-demo-videos.py` on macOS.

Profile setup stores Opinion examples separately from signed opinions, creates
classrooms owned only by the current profile, and adds the two video courses
as editable drafts. Videos are copied into the local content store and pinned
for offline use. Existing course/classroom edits survive repeat setup. The
retired seed's fake people, proofs, messages, and activity history are excluded.
