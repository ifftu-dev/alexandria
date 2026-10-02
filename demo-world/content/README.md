# Demo course corpus

Non-authoritative source material. **Instructor → My courses → Load example
courses** explicitly imports the seven full courses as profile-owned drafts,
using inline text and quizzes only. Startup does not import them. Re-import
preserves edits; external media and plugin/video tutorials are excluded.
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

The separate **Load plugin showcase** action imports `course_plugin_demo`,
resolves its six lessons to the current embedded plugin CIDs/versions, repairs
legacy starter-code newlines, and installs the bundled course plugins.
