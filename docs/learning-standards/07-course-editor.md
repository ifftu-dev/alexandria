# Adding a package in the course editor

Triage: done
Repos: alexandria
Part of: docs/learning-standards-spec.md
Blocked by: docs/learning-standards/06-player.md

Spec: user stories 43 to 50; Distribution → second bullet; Credentials.

- An instructor adds a standards package to a course as one element.
- Before adding, the editor shows the package's title, standard, edition and lesson structure. A refused package shows its refusal reason in words an instructor can act on.
- Adding a package to a course that is or will be public warns that every device taking the course will redistribute it and asks the instructor to confirm they may pass the content on.
- The editor refuses to make a package element the assessed basis of a credential.
- Preview runs the package as a learner would see it, without recording an attempt against a real enrolment.

Done when: interface tests cover the summary, each refusal message, the redistribution confirmation, the assessed-element refusal and preview; a published course carries the package by content hash.

## Notes

2026-11-20: built.

- Adding: the outline offers a "Standards package" type. Choosing it opens a picker (`src/components/composer/PackagePicker.vue`) that takes a .zip from the file dialog, imports it with `package_import` (checks and keeps it as a draft archive) and shows title, standard, edition, size and the lesson tree, headings included, before anything joins the course. The element is created on confirm with the package's hash and, unless the instructor typed one, the package's title. An existing package element's editor (`editors/PackageEditor.vue`) shows the attached package and replaces it through the same picker.
- Refusals: every import refusal code, plus `package_result_self_reported` and `not_a_package`, maps to an instructor-facing sentence (`packageMessages.ts`); the host's own detail is shown beneath it.
- Redistribution: the warning and a confirmation checkbox show whenever the course is or will be public. Every published course is public until private courses ship, so today that is always; `courseWillBePublic()` is the one place to change when course visibility exists.
- Assessed-element refusal: a package element's editor has no skill tag picker and no grading controls, and says why. The host refuses changing a package element's type or turning another element into a package (`update_element`), a plugin or plugin configuration on a package element, and attaching a hash that is not an imported package (`create_element`/`update_element`); the refusals use the database triggers' code `package_result_self_reported` (and `not_a_package`), and the editor explains them. As ticket 05 recorded, the completion policy has no field naming an element and the signed document carries no grading data, so there is nothing for document validation to refuse; the refusal lives in the editor, the element commands and the database.
- Preview: chosen as a preview attempt mode, not a throwaway enrolment. A throwaway enrolment would write an enrolment learning record, sync through device sync and reach the guardian mirror. `crate::packages::preview` holds a preview's run-time state in memory only (the attempt rules from the shared crate, same identifier derivation, same initialisation), limited to the course's author, at most four at once, cleared on profile lock. Commands `package_preview_{open,fresh,initialise,commit,activities,terminate}`; the player takes a `preview` prop and sends its attempt commands there (`packagePreview.ts`). Nothing is written: no attempt, element progress, enrolment or learning record (host-seam test).
- Publishing: the publish command's work moved into `prepare_course_document_core` (Tauri-free, same file) so it can be tested. When publishing, every package element must name a package, and each archive goes through `packages::publish` (served store, never-evicted pin) before the document is signed and announced. Local preparation leaves archives as draft files. Host-seam tests: the published document carries the package by content hash, the store holds the archive with a `course` pin, an element with no package refuses publishing, local preparation serves nothing.
- Package file from disk: `package_open_file` now also enrols the learner locally (signs and stores the draft's document for this device without a catalog entry, as `prepare_local_course` does, then enrols) and returns `OpenedPackageCourse` with the enrolment id; opening the same file again returns the same enrolment. The archive stays a draft file the node cannot serve. The courses page has an "Open a package file" button that opens the player. The signed draft document itself is in the local store, unannounced, as for any locally prepared draft.
- Tests: Rust host seam in `commands/courses.rs` (publishing), `commands/packages.rs` (opening a file), `commands/elements.rs` (type and attachment refusals), `packages/preview/tests.rs`. Interface: `tests/desktop-ui/course-editor-package.spec.ts` (screen `course-editor`, backend `courseEditorBackend.ts`, package origin helper `packageOrigin.ts`), 5 tests in Chromium and WebKit.
- New English strings (`instructor.package.*`, `instructor.elementTypes.package`, `instructor.compose.blockerUnattachedPackages`, `learn.packageFile.*`) are in `PENDING_TRANSLATION` with a reason; the redistribution confirmation should be prioritised in the translation pass.
- Seen while testing, not caused here: `package-player.spec.ts` fails intermittently when repeated (`--repeat-each=3`), with and without this ticket's player change.
- Follow-ups: SCORM 2004 preview needs ticket 10's sequencing state held in the preview too; a package picked and then cancelled or replaced leaves its draft archive in the profile's packages directory with no element using it (nothing collects unused drafts yet).

Proposed doc edits, not made here:

- `docs/learning-standards-spec.md` → Credentials: say the refusal is enforced by the element commands and database triggers, since the completion policy names no element.
- `docs/learning-standards-spec.md` → Package storage and import, last bullet: a package file opened from disk is enrolled locally on a device-only signed document.
- The glossary and architecture documents: preview attempts (in memory, author only).

### Review (2026-11-21)

- A package picked and then cancelled or replaced no longer leaves its draft archive for good: profile start sweeps draft packages no element and no attempt refers to (`packages::sweep_unused_drafts`, see ticket 03). No cancel command was added; the sweep covers both cases.
