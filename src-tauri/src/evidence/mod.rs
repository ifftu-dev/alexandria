// Post-migration 040 (VC-first cutover):
//   - `aggregator` was deleted; auto-earned VCs replace skill-proof
//     aggregation (see `commands/auto_issuance`).
//   - exact course-completion endorsements live at `commands::attestation`
//     (data: `course_completion_endorsements`).
//   - `reputation` was rebuilt against `credentials`.
pub mod reputation;
pub mod thresholds;
