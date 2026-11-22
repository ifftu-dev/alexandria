# Demo course corpus

Non-authoritative source material. Profile creation, mnemonic restore, and
unlock import the nine full courses as profile-owned drafts using inline
text and quizzes, plus Plugins Showcase and the bundled plugins.
Existing course edits are preserved. The seven video tutorials and external
placeholder media are excluded. **Instructor → My courses → Load example
courses** remains available to restore missing example drafts.
Nothing in this source corpus is authoritative: there are no signed rows, identities,
credentials, enrolments, opinions or governance records.

- `courses.json` — 17 example courses and tutorials with their chapters,
  elements, inline lesson content, skill tags and video chapters. It was
  exported from the retired startup seed with its fabricated authorship,
  publication, chain and content-address fields removed. Courses marked
  `provenance: "ai_generated"` (every course except Plugins Showcase) are AI-generated example
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

## Plugins Showcase

Open **Instructor → My courses → Load plugin showcase** to open or create the
showcase draft. A fresh import includes seven lessons and resolves each to its
current bundled plugin. If an existing draft predates Workout Assist, add a
plugin element and select **Workout Assist** in its editor.

### Workout Assist

1. In the course player, open **Workout Assist → Practise a standing arm raise**
   and choose **Replay demonstration** to see the example before enabling your camera.
2. To teach your own movement, open the lesson in the composer. Allow on-device
   inference and camera access, start the camera, and step back until your joints
   are visible. Choose **Record movement**, perform one repetition, and choose
   **Use this take** or let the 12-second recording finish.
3. Replay the take and choose **Save** in the composer. Publishing the course
   shares the recorded poses and the silent video if the device captured one.
4. As a learner, start the camera and follow the demonstration from the same
   angle and at the same pace. When a joint differs by at least 15°, the plugin
   highlights the largest visible difference and shows both angles. Choose
   **Finish practice** after receiving feedback.

The plugin ships with the model for offline pose tracking. [MoveNet model source](https://www.kaggle.com/models/google/movenet/tfJs/singlepose-lightning/4) · [Author-mode configuration event](../../docs/plugins.md#authoring-lesson-content)

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

## English for Hindi Speakers videos

`english-videos.json` holds the scripts for the 21 narrated lessons in
`course_english_hindi_101`, and `videos/eng_v_*.mp4` holds the rendered clips.
Hindi carries the explanation; English voices speak only the English being
taught. They are AI-generated with macOS synthetic voices (Lekha, Samantha,
Rishi), and the Hindi still needs a native speaker's review. Profile setup
copies and pins them like the other bundled videos. Regenerate on macOS with
`node scripts/generate-english-videos.mjs [video-id ...]` (needs FFmpeg and the
Playwright Chromium build); it records each duration back into the JSON.
