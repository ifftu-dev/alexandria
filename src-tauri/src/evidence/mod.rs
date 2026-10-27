// Post-migration 040 (VC-first cutover):
//   - `aggregator` was deleted; auto-earned VCs replace skill-proof
//     aggregation (see `credentials/auto`).
//   - exact course-completion endorsements live in `completion`
//     (data: `course_completion_endorsements`).
//   - `reputation` was rebuilt against `credentials`.
pub mod completion;
pub mod reputation;
pub mod thresholds;
