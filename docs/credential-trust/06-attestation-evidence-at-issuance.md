# 06 Attestation evidence at issuance

Triage: done
Type: task
Repos: alexandria
Part of: alexandria/docs/credential-trust-spec.md
Blocked by: 05

Spec sections: *Attestation evidence in the credential*, stories 18–20, 22, 24–25.

## What to build

A credential issued against an attested integrity session carries attestation evidence, bound to the credential when the platform allows.

- At issuance, after the integrity assertion is built and before signing, compute the binding digest (ticket 05) and obtain platform evidence over it: a fresh App Attest assertion from the session's attested key on iOS; a fresh Play Integrity token with the digest as nonce on Android; for a companion-phone session, a request to the phone over the companion link for the same, before the link closes.
- Put the evidence in the credential's `evidence` as credential-bound. If evidence over the digest cannot be obtained, carry the session's existing evidence as session-bound. Never upgrade later.
- Obtaining evidence goes through a port so tests use a fake attester; the native calls stay behind it.
- Keep per-session attested keys (no device linkage across credentials).
- Raise credential size limits deliberately if evidence does not fit, and update the limit vectors; never truncate.
- Bundles carry the evidence as part of the credential.
- The learner can see, from the issuance result, when evidence could only be session-bound and why.

## Acceptance criteria

- [x] With a fake attester, an attested session issues a credential whose evidence verifies `verified_credential_bound` through the library.
- [x] With the fake attester failing, the credential carries session-bound evidence that verifies `verified_session_bound`.
- [x] An unattested session issues a credential with no attestation evidence.
- [x] Companion sessions request evidence from the phone; a test drives the request over the companion message types with a fake phone.
- [x] Grading still commits atomically: a failure to obtain evidence never fails issuance.

## Pointers

- Native capture and assertion: `src-tauri/src/sentinel/attestation/native.rs` (`assert_commitment`).
- Companion link: `src-tauri/src/sentinel/companion/` (`transport.rs`, `session.rs`).
- Issuance: `src-tauri/src/credentials/issue.rs`; assessment grading: `src-tauri/src/commands/assessment.rs`.
- Size limits: `crates/alexandria-verify/tests/credential_limits.rs`, `limit_vectors.rs`.

## Notes

2026-11-16: built.

- Port: `sentinel::attestation::BindingAttester` (`bind(session_id, record, digest) -> Result<proof, reason>`). `NativeAttester` calls the new `native::bind_digest` (iOS: assertion from the session's App Attest key with `clientDataHash = SHA-256(digest)`; Android: a fresh Play Integrity token with the digest as nonce; desktop: none). `companion::transport::PairedAttester` asks a paired phone over the link and uses the native attester for every other session.
- Issuance: `credentials::issue_with(conn, issuer, req, now, &EvidenceSource)` returns `Issued { credential, evidence: EvidenceBinding }`; `issue()` keeps its signature and uses `EvidenceSource::native()`. The acquisition lives in `credentials/evidence.rs` (`attach`), called in `issue_with` after the envelope is built and before signing; the change to `issue.rs` is that call, the trust source and the return type. The policy is still evaluated before evidence is obtained, so a refused credential costs no platform call; ticket 08 can move `attach` ahead of the policy when it judges the verified assurance level.
- Decision: evidence is checked with the verifier library before it is signed. The session's stored attestation is carried only when it verifies or is unverifiable on this build (no trust input, e.g. Android without Play keys); one that fails is not carried. A credential-bound proof that does not verify falls back to session-bound with the reason. Issuance never signs evidence a verifier would reject, and an attester error never fails issuance.
- `EvidenceBinding` is `none`, `credential` or `session { reason }`. Grading reports it as `GradeResult.attestation_evidence`; the assessment result screen shows an English-only note when it is session-bound (`learn.assessment.sessionBoundEvidence`, listed as pending translation).
- Companion: new request `Bind { session_id, digest_hex }` on the link; the phone answers with its `PhoneSigner.bind_digest`. `CompanionHub::join` takes a `PhoneSigner` (root asserter and digest binder) in place of the bare asserter. At the end of an `assessment` session the link now stays up for 10 minutes (`close_after`) so grading can ask the phone; interview sessions still close at once. The desktop waits at most 15 s for the phone, holding the database thread while it does.
- Wiring: `assessment_grade` uses `PairedAttester`; adaptive finalize uses the native source (a paired phone's adaptive session issues session-bound) and reports the binding too. Other `issue()` callers get native evidence.
- Size: evidence at generous platform sizes (12 KiB attestation object, 8 KiB tokens) is under a quarter of the 256 KiB credential limit and the 64 KiB string limit. No limit changed; limit vectors untouched.
- Per-session keys unchanged: each session generates its own App Attest key.

2026-11-16 (review fix): evidence is no longer obtained inside the grading write transaction. Grading and adaptive finalize rehearse in a transaction that is rolled back (`credentials::RecordingAttester`), ask the attester with no database job running, then commit with the answer (`credentials::PreparedEvidence`); a credential whose binding digest changed in between is session-bound with that reason. The 15 s phone wait no longer holds the database thread.
