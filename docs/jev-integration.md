# Optional Jev integration

Jev is off by default. It proposes structured checks; it does not generate course prose, grade assessments, establish proficiency, issue credentials, change prerequisites, or make governance decisions.

## App configuration

Open **Instructor → AI settings → Optional Jev checks**. Configuration and the provider key belong to the active profile. Keys use the existing SQLCipher-backed Studio secret store and are never returned to the frontend. “Remove saved key” erases the saved value. Choose the individual tasks, enable hosted processing, and save. Shadow mode also sends selected content to TypeSafe AI. The button beside a draft/document makes that disclosure before a request.

- Job descriptions and document claims: request a check, inspect the quotation and relation, then add a suggestion to the existing confirmation list. Added suggestions are initially unchecked. A document remains a self-asserted claim.
- Studio: check an editable output against selected course sources, outcomes, prerequisites, and worked-example expectations. Checks never apply a draft. Existing review/apply conflict checks still control writes.
- Search: local results appear first. Optional relevance checks use up to 20 candidates per domain; local ordering remains on failure and once keyboard navigation begins. This cannot recover candidates absent from retrieval.
- Tutor: an explicit check uses only the current locally available text lesson and the last two conversation messages. Existing tutor access rules and assessment exclusions apply. It neither generates a reply nor reads answer keys. CID-only lessons keep the existing tutor behavior without this optional check.
- **Goals → Add a goal → Learning goal** accepts a free-text learning objective, offers local matching and an explicit Jev review using the `learning_goal` rubric. Model suggestions remain unchecked until selected. Source, tab and profile changes discard pending local/model results. Job links retain the local flow.

Assist requires a reviewed task-specific evaluation and the operator environment variable `ALEXANDRIA_JEV_APPROVED_TASKS` (comma-separated `job_description,learning_goal,document_claim,studio_review,search,tutor`). This is an operator attestation, not an automatic claim that evaluation passed. Enable only tasks and source languages covered by the approved report. No task is approved by this change. Shadow checks return no model-derived changes to the UI.

Profile changes and settings revisions invalidate pending responses. Learning checks verify that candidate definitions still match the bundled snapshot before returning a provider result. Source edits invalidate the visible review. No database lock spans network I/O. Model input excludes credential records and assessment keys.

## Local learning shadow comparisons

Storage is a separate profile opt-in, off for existing and new settings. Enable **Keep local learning shadow comparisons** to retain JD, learning-goal and document-claim comparisons in the SQLCipher profile database (migration 4). Each sample includes submitted source text, candidate IDs, local matcher suggestions, validated provider response/usage, the source/taxonomy-bound record, and elapsed time. Search, Studio and tutor shadow results are not retained.

At most 100 samples remain. Seven-day expiry is enforced on settings reads, decision preparation, saving and export; a locked or unused profile is not decrypted by a background retention worker. Turning mode off, disabling cloud permission or retention deletes the samples. **Delete learning comparisons** also invalidates pending checks so an old request cannot repopulate the table. Samples are local only, outside synced settings and the audit chain. Deleting the profile removes its database.

**Export learning comparisons** produces a local JSON download with `schema_version: 1`, `independently_labelled: false` and `samples`. It contains source text. Exported files are outside app retention and must be deleted separately. They are review material, not a labelled evaluation corpus: preserve the taxonomy digest, add independent human labels and the evaluation split/manifest before converting to the evaluator's JSONL. Local matcher suggestions are raw baseline output, not ground truth. No hosted evaluation is triggered by export.

The new goal entry and retention controls currently use English fallback pending translation review. Assist task/language approvals remain unchanged.

## Shared Cloud/app vocabulary

`alexandria-learning-contracts` is an I/O-free crate defining exact Bloom values, versioned taxonomy snapshots, byte-offset evidence, probability validation, and source/taxonomy-bound decision records. `alexandria-verify` 0.2.0 uses its Bloom ordering. Cloud consumes identical release-candidate source snapshots, with checksum validation; packages have not been published.

Export the same snapshot the app uses:

```sh
cargo run -p alexandria-learning-contracts --example export_taxonomy -- \
  src-tauri/resources/networks/preprod.json bootstrap/public_taxonomy.json > taxonomy.json
```

The exporter takes the network ID from the profile, retains canonical IDs and prerequisite edges, uses `bundled-v1`, sorts the content, and computes a SHA-256 digest. Review authority and redistribution terms before activating an artifact in Cloud. Cloud's documented import requires its exact digest and network ID. Do not map `skill_kubernetes` to `skill_k8s`, or any other legacy ID, automatically. Existing signed talent records stay byte-for-byte intact.

## Transport and evaluation

`alexandria-decisions` is separate from the contract and text provider. It pins `jev-1.13.0` and `learning-v1`; validates Choice, Score and Noul responses; limits encoded requests to 32,000 bytes and 128 questions; permits four in-flight calls per client; and conservatively reserves at most 200,000 input-budget units per scope/hour. The budget uses encoded bytes plus overhead, not measured billing tokens. Requests have a five-second total deadline, two-second connection deadline, no redirects and no automatic retries. Oversize, unavailable, malformed, or contradictory results fall back to local behavior. There is no cross-profile result cache. An API key/settings change creates a new client budget; these limits are process-local admission controls, not a billing ledger.

Offline evaluation never sends data:

```sh
cargo run -p alexandria-decisions --example evaluate -- manifest.json taxonomy.json examples.jsonl
```

The manifest fields are `dataset_id`, `provenance`, `split` (`development`, `held_out`, or `synthetic`), `independently_labelled`, `language`, `task`, `taxonomy_digest`, `model`, `rubric_version`, and preselected `thresholds`. Each JSONL example supplies `id`, `source`, `candidates`, human `expected` labels, `baseline` labels, a saved `response` or `record`, and `latency_ms`. Labels contain `skill_id`, `relation`, and nullable canonical `bloom`. Missing responses count as failures and missed positives. Reports contain candidate recall counts, precision/recall counts, necessity errors, Bloom overstatement/absolute error, threshold coverage counts, token usage when available, and latency percentiles. They always return `rollout_approved: false`.

Human held-out data, task/language calibration, measured production latency/cost, and owner rollout approval remain required. Synthetic tests establish contract behavior, not model quality. Live evaluation is a separately configured action; no paid inference or private-data upload was performed during implementation.

API and model references: https://docs.typesafe.ai/api and https://docs.typesafe.ai/models (checked 2026-09-30).
