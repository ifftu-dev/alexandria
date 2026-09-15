// Post-migration 040 (VC-first cutover):
//   - `aggregator` was deleted; auto-earned VCs replace skill-proof
//     aggregation (see `commands/auto_issuance`).
//   - `attestation` was rebuilt around completion-witness gating and
//     lives at `commands::attestation` (data: `completion_attestation_*`).
//   - `reputation` was rebuilt against `credentials`.
pub mod reputation;
pub mod taxonomy;
pub mod thresholds;
