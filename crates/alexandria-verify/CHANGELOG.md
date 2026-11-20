# Changelog

## 0.8.0 (unreleased)

Assessment hardening: a credential says who held the answers, a desktop
session backed by a paired phone gets its own level, and an organisation's
policy can require issuer-held answers and a rung that its evidence shows.

### Breaking changes

- **The integrity assertion states answer custody.**
  `IntegrityAssertion::answer_custody: Option<AnswerCustody>`
  (`answerCustody`: `device` or `issuer`, omitted from JSON when absent). An
  assertion without it reads as `device` (`IntegrityAssertion::answer_custody()`).
  A struct literal of `IntegrityAssertion` must set it; assertions without it
  serialize and hash as before. This is the assertion format change; the
  assertion carries no format version field of its own, so this crate's
  version is it.
- **`companion_attested`.** `VerifiedAssuranceLevel` gains
  `CompanionAttested`, ordered `local` < `companion_attested` <
  `device_attested`, with `ALL` and `parse`.
  `AttestationOutcome::VerifiedCredentialBound` and `VerifiedSessionBound`
  gain `companion: bool` (omitted from JSON while false), set when a paired
  phone produced the evidence. Companion credential-bound evidence verifies
  as `companion_attested`, never `device_attested`. Patterns naming the
  fields need `companion` or `..`.
- **Companion evidence no longer carries a policy floor.** A floor's device
  attestation signal counts only for verified evidence from the attempt's own
  device; a paired phone's evidence, either binding, says nothing about the
  desktop.
- **New issuance policy requirement.** `IssuancePolicy::required_answer_custody:
  Option<AnswerCustody>` (omitted from the stored shape while unset):
  `issuer` is met only by `issuer`, `device` by either. A struct literal must
  set it. `PolicyRefusal::CustodyBelowRequired` (`custody_below_required`) is
  judged first: the order is custody, status, score, flags, assurance, floor,
  rung, probes. A required `device_attested` is not met by
  `companion_attested`; a required `companion_attested` is met by either.
- **The required rung is judged on the strict ladder.** A stated rung of
  `probed` or above counts only beside a probe step with outcome `satisfied`,
  at least one satisfied probe and none failed or skipped (its policies are
  not judged, so a `reseed` policy still counts); otherwise the rule credits
  `process`. `RungBelowMinimum::rung` names the credited rung.
- **New result field.** `VerificationResult::custody_defects:
  Vec<CustodyDefect>` (omitted from JSON while empty); any defect rejects.
- **Trust classification.** `TrustInvalidReason` gains
  `IssuerCustodySelfIssued`, `HostedAttemptSelfIssued`, `HostedAttemptInvalid`
  and `HostedAttemptCustodyMismatch`; a result whose `custody_defects` differ
  from the credential's is `InconsistentVerificationResult`.
  `TRUST_CALCULATION_VERSION` is 5.
- **Coverage statements.** `CURRENT_VERSION` is `"3"` and `VERSIONS` gains
  it. Every published version-1 and version-2 digest still resolves.

### Added

- **Answer custody** (`custody`, `integrity::AnswerCustody`): `custody_defects`
  reports, in order, `issuer_custody_self_issued`, `hosted_attempt_self_issued`,
  `hosted_attempt_invalid` and `hosted_attempt_custody_mismatch`, judged on
  the credential alone (self-issued: the issuer DID is the subject's).
- **Hosted attempt evidence** (`custody::HostedAttemptEvidence`,
  `HOSTED_ATTEMPT_EVIDENCE_TYPE`): the `evidence` entry an organisation's role
  credential from a hosted attempt carries, with `attemptId`, `answerCustody`
  and `gradedBy` (`issuer`), the issuer's `probe` summary, and the
  `attestation` outcome and `verifiedAssuranceLevel` it found (`new`, `find`,
  `into_entry`, `assurance_level`, `states_a_hosted_attempt`). Valid only on
  an issuer-signed credential stating `issuer` custody.
  `policy::evaluate_hosted` judges a role's policy from it: its level, its
  outcome for a floor, and its probe step for the probe policy and the rung.
- **Coverage statement version 3**: below `live`, a line says a second device
  or person was not ruled out; the probe lines say the follow-up questions
  were ones the learner had not already answered, with a time limit fixed by
  the item.
- **Vectors**: credential vectors 24–29 (answer custody, hosted attempts, a
  version-3 statement), attestation vector 13 (companion session-bound),
  issuance policy vectors 27–51 (companion level, custody, the rung at every
  step and its probe evidence, companion floors, hosted attempts). The
  independent JavaScript verifier checks custody.

## 0.7.0

Credential trust: what a credential says about how it was earned becomes
checkable where the platform allows it, organisations can put their own
signature on a result, and a credential can be suspended as well as revoked.

### Breaking changes

- **`credentialStatus` is one entry or a list.**
  `VerifiableCredential::credential_status` is now
  `Option<CredentialStatuses>` (`One` or `Many`). Credentials issued by
  Alexandria carry a list: a `revocation` entry and a `suspension` entry, each
  into a Bitstring Status List of that purpose, at the same index. The form
  read is the form written back, so a credential signed with a single object
  still verifies. Use `status_entries()` / `status_entry(purpose)` on the
  credential, or `entries()` / `of_purpose(purpose)` on the status.
- **Suspension comes from the suspension list only.**
  `VerificationStore::suspension` is removed; the suspension entry's bit,
  read through `status_list_bits`, is the only suspension state verification
  reads. `VerificationPendingReason::SuspensionStateUnavailable` is replaced
  by `SuspensionListMissing` and `SuspensionListUnavailable`.
- **Key rotation is removed.** `VerificationStore::key_at(did, at)` is now
  `registered_key(did)` and is consulted only for a DID method that is not
  self-resolving; a `did:key` never asks the store, so an unavailable store no
  longer adds `IssuerKeyUnavailable` for one. `KeyRegistryEntry` loses
  `valid_from`, `valid_until` and `rotated_by`. Vector
  `08-rotated-issuer-key` is replaced by `08-did-key-ignores-registry`.
- **The issuer may be an object.** `VerifiableCredential::issuer` is now
  `CredentialIssuer`: a DID string, or an `IssuerObject` whose `id` is the DID
  and whose other properties (an organisation's `name` and `domain`) are kept
  as received, so the proof covers them. `id()` / `as_str()` return the DID;
  `From<Did>` and `PartialEq<Did>` keep call sites short. `sign_credential`
  keeps a preset issuer object whose `id` is the signing DID.
- **New envelope fields.** `evidence: Vec<EvidenceEntry>` (VC Data Model 2.0
  `evidence`) and `related_resource: Vec<RelatedResource>`, both omitted from
  JSON when empty. A struct literal of `VerifiableCredential` must set them.
- **New integrity assertion field.** `IntegrityAssertion::camera_coverage`
  (`cameraCoverage`, omitted from JSON when absent): the fraction of
  committed windows that carried camera checks, set only when the profile
  includes the camera. A struct literal of `IntegrityAssertion` must set it;
  assertions without it serialize and hash as before.
- **New result fields.** `VerificationResult` gains `attestation`
  (`AttestationOutcome`), `verified_assurance_level`
  (`VerifiedAssuranceLevel`) and `coverage_statement_mismatch` (omitted from
  JSON while false).
- **`CredentialRequest` is no longer `Eq`.** It gains a field holding an
  `IssuancePolicy` (floating-point bounds), so it derives `PartialEq` only.
  A struct literal must set `issuance_policy`.
- **Trust classification.** `CredentialTrust::VerifiedSelfClaim` gains
  `attestation`; `VerifiedIssuerSigned` gains `attestation` and
  `organisation`. `TrustInvalidReason::InvalidAttestation` is new, and
  `TRUST_CALCULATION_VERSION` is 3.

### Added

- **Device attestation evidence** (`attestation`): an `evidence` entry of type
  `DeviceAttestationEvidence` carrying Apple App Attest or Google Play
  Integrity material, credential-bound (over the credential's
  `binding_digest`: subject, claim, integrity assertion and commitment root)
  or session-bound. `verify_evidence` reports an `AttestationOutcome`
  (`not_supplied`, `verified_credential_bound`, `verified_session_bound`,
  `unverifiable`, or `invalid` with a reason); invalid evidence rejects the credential.
  `verify_credential_with_attestation` takes the trust inputs
  (`AttestationTrust`); `verify_credential` keeps its signature and passes
  none. Only verified credential-bound evidence sets `verifiedAssuranceLevel`
  to `device_attested`, whatever level the integrity assertion states. The
  certificate and token checks are behind the `attestation` feature; without
  it evidence parses and verifies as `unverifiable`. The `test-support`
  feature adds deterministic synthetic App Attest chains and Play Integrity
  tokens for tests.
- **Organisation backing** (`org`): `organisation_backing(credential,
  descriptor)` reports whether the organisation's DNS-published descriptor
  lists the issuer key: `verified { name, domain }`, `key_not_listed`,
  `wrong_domain`, `invalid_descriptor` or `descriptor_not_supplied`. Backing
  never changes the verification result.
  `trust::classify_credential_with_organisation` and
  `CredentialTrust::self_issued()` carry it into the classification.
- **Coverage statement registry** (`coverage`): every published statement
  text, content-addressed by a `blake3:` digest. `statement`, `statements`,
  `resolve` (any published version, offline), `CURRENT_VERSION` (`"2"`) and
  `VERSIONS`. A credential names its statement in `relatedResource` (`id`,
  `digestSRI`, `mediaType`); the verifier rejects one whose named statement
  is not the integrity assertion's `coverageStatementDigest`.
- **Shared issuance policy** (`policy`, `integrity`): `IssuancePolicy`,
  `ProbePolicy`, `PolicyRefusal` (with a stable `code()`) and `evaluate`, the
  one rule an issuing app and a hosted service both judge, returning the first
  unmet requirement. A required assurance level is judged against the
  verified level, never the stated one. A degraded signal never carries a
  policy floor, except the camera when the assertion states `cameraCoverage`
  at or above `MIN_CAMERA_COVERAGE` (0.9, judged by
  `integrity::camera_coverage_counts`); without the field a degraded camera
  does not count. `TrustProfile`, `ProfileSignal`, `VerificationRung`,
  `PolicyFloor`, `evaluate_floor` and `MIN_CAMERA_COVERAGE` moved here from
  the studio crate.
- **Assessed credential evidence** (`assessed`): `AssessedCredentialEvidence`
  (`ASSESSED_CREDENTIAL_EVIDENCE_TYPE`), the `evidence` entry a role credential
  carries naming the credential it was issued on by `credentialId` and
  `integrityHash`, with the `attestation` outcome and
  `verifiedAssuranceLevel` the issuer established for it (`new`, `find`,
  `into_entry`, `assurance_level`). `policy::evaluate_assessed` judges a role
  credential's policy from that entry: its verified level, counted only as
  far as its own outcome verifies it, and its outcome for a floor's device
  attestation.
- **The policy in the request** (`exchange`): `CredentialRequest` gains an
  optional `issuance_policy`, the policy the organisation will judge the
  answer against, so the candidate sees it before starting. It is covered by
  the organisation's request proof and the holder's presentation signature;
  `verify_share` refuses a presented request whose policy differs from the
  expected one. Omitted from JSON when absent, so a request without one
  serialises byte-identically to 0.6.0. A request never carries the
  device-local `probe_connection_id`: `CredentialRequest::check` refuses one
  (called by `present_credential` and `verify_share`), and
  `IssuancePolicy::for_request` builds the policy without it.
- **Vectors**: credential vectors 13–23 (coverage statements, suspension,
  issuer objects, organisation backing), and separate attestation and
  issuance-policy vector sets with their own runners (policy vectors 20–22
  judge role credentials from their assessed-credential entry; 23–26 judge a
  camera floor from the stated camera coverage). The independent
  JavaScript verifier runs from any directory.

### Fixed

- Documentation: the integrity assertion's statuses include `review`;
  `device_attested` is achievable and reported as verified only from
  evidence; the credential proof is a Data Integrity proof
  (`eddsa-jcs-2022`), not a JWS; no key-rotation language remains.

## 0.6.0 and earlier

See the repository history.
