<script setup lang="ts">
import { ref, onMounted, computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRoute, useRouter } from 'vue-router'
import { AppBadge, AppButton, AppAlert } from '@/components/ui'
import { useCredentials } from '@/composables/useCredentials'
import { anchorBadge } from '@/components/credential/anchorStatus'
import { explorerTxUrl } from '@/utils/cardanoExplorer'
import CoverageStatementView from '@/components/integrity/CoverageStatementView.vue'
import { secondDeviceLine } from '@/utils/sentinel/secondDevice'
import { gradedBy } from '@/utils/hostedEvidence'
import type {
  AttestationPlatform,
  CredentialAnchorState,
  CredentialTrustView,
  EndorsementMismatch,
  EndorsementOutcome,
  TrustInvalidReason,
  VerifiableCredential,
  VerificationPendingReason,
  VerificationResult,
} from '@/types'

const { t } = useI18n()
const route = useRoute()
const router = useRouter()
const api = useCredentials()

const credentialId = computed(() => route.params.id as string)

const credential = ref<VerifiableCredential | null>(null)
const verification = ref<VerificationResult | null>(null)
/** What the trust command states; the page renders it and judges nothing. */
const view = ref<CredentialTrustView | null>(null)
const trust = computed(() => view.value?.trust ?? null)
/** Below `live`: the statement's own line that a second device or person was not ruled out. */
const secondDevice = computed(() => secondDeviceLine(view.value?.coverage_statement))
/** Who computed the score: the issuer, from a hosted attempt entry, or the device. */
const grader = computed(() => (credential.value ? gradedBy(credential.value) : null))
const anchor = ref<CredentialAnchorState | null>(null)
const retryingAnchor = ref(false)
const loading = ref(true)
const verifying = ref(false)
const revoking = ref(false)
const statusPublish = ref<{ variant: 'success' | 'warning'; message: string } | null>(null)
const revokeReason = ref('')
const showRevoke = ref(false)
const error = ref<string | null>(null)

onMounted(async () => {
  loading.value = true
  const c = await api.get(credentialId.value)
  credential.value = c ?? null
  if (c) {
    // Auto-verify on load — gives the user immediate signal.
    verification.value = (await api.verify(c)) ?? null
    await loadTrust()
  } else {
    error.value = api.error.value ?? t('credentials.detail.notFound')
  }
  loading.value = false
})

function signalNames(signals: string[]) {
  return signals.map(s => t(`sentinel.trust.signal.${s}`)).join(', ')
}

/** Whole percent, never rounded up across a threshold such as the 90% the camera needs. */
function coveragePercent(fraction: number) {
  return Math.floor(fraction * 100 + 1e-9)
}

async function reverify() {
  if (!credential.value) return
  verifying.value = true
  verification.value = (await api.verify(credential.value)) ?? null
  await loadTrust()
  verifying.value = false
}

async function loadTrust() {
  view.value = (await api.trust(credentialId.value)) ?? null
  anchor.value = (await api.anchor(credentialId.value)) ?? null
}

async function retryAnchor() {
  retryingAnchor.value = true
  anchor.value = (await api.retryAnchor(credentialId.value)) ?? anchor.value
  retryingAnchor.value = false
}

const anchorBody = computed((): string => {
  const value = anchor.value
  if (!value) return t('credentials.anchor.body.none')
  switch (value.status) {
    case 'confirmed':
      return t('credentials.anchor.body.confirmed', { time: value.confirmed_at ?? '' })
    case 'pending':
      return value.last_error
        ? t('credentials.anchor.body.retrying', { error: value.last_error })
        : t('credentials.anchor.body.pending')
    case 'submitted':
    case 'outcome_unknown':
      return t('credentials.anchor.body.submitted')
    case 'failed':
      return t('credentials.anchor.body.failed', { error: value.last_error ?? '' })
  }
})

async function revoke() {
  if (!credential.value?.id) return
  revoking.value = true
  await api.revoke(credential.value.id, revokeReason.value || t('credentials.detail.revokeDefaultReason'))
  // The backend pushes the list inline; this reports the outcome and retries
  // anything that stayed pending.
  const report = await api.publishStatusLists()
  if (report && report.errors.length > 0) {
    statusPublish.value = { variant: 'warning', message: t('credentials.detail.statusPublishFailed', { detail: report.errors[0] }) }
  } else if (report && report.published[0]) {
    statusPublish.value = { variant: 'success', message: t('credentials.detail.statusPublished', { host: new URL(report.published[0]).origin }) }
  }
  revoking.value = false
  showRevoke.value = false
  // Re-verify to surface the revoked flag.
  verification.value = (await api.verify(credential.value)) ?? null
  await loadTrust()
}

function invalidReasonLabel(reason: TrustInvalidReason): string {
  switch (reason) {
    case 'inconsistent_verification_result': return t('credentials.detail.trustReason.inconsistent')
    case 'issuer_unresolved': return t('credentials.detail.trustReason.issuerUnresolved')
    case 'invalid_signature': return t('credentials.detail.trustReason.invalidSignature')
    case 'subject_not_bound': return t('credentials.detail.trustReason.subjectNotBound')
    case 'invalid_status_reference': return t('credentials.detail.trustReason.invalidStatusReference')
    case 'revoked': return t('credentials.detail.trustReason.revoked')
    case 'expired': return t('credentials.detail.trustReason.expired')
    case 'suspended': return t('credentials.detail.trustReason.suspended')
    case 'superseded': return t('credentials.detail.trustReason.superseded')
    case 'integrity_anchor_missing': return t('credentials.detail.trustReason.anchorMissing')
    case 'type_not_allowed': return t('credentials.detail.trustReason.typeNotAllowed')
    case 'invalid_attestation': return t('credentials.detail.trustReason.invalidAttestation')
    case 'issuer_custody_self_issued': return t('credentials.detail.trustReason.issuerCustodySelfIssued')
    case 'issuer_custody_unproven': return t('credentials.detail.trustReason.issuerCustodyUnproven')
    case 'hosted_attempt_self_issued': return t('credentials.detail.trustReason.hostedAttemptSelfIssued')
    case 'hosted_attempt_invalid': return t('credentials.detail.trustReason.hostedAttemptInvalid')
    case 'hosted_attempt_custody_mismatch': return t('credentials.detail.trustReason.hostedAttemptCustodyMismatch')
  }
}

function pendingReasonLabel(reason: VerificationPendingReason): string {
  switch (reason) {
    case 'issuer_key_missing': return t('credentials.detail.trustPending.issuerKeyMissing')
    case 'issuer_key_unavailable': return t('credentials.detail.trustPending.issuerKeyUnavailable')
    case 'status_list_missing': return t('credentials.detail.trustPending.statusListMissing')
    case 'status_list_unavailable': return t('credentials.detail.trustPending.statusListUnavailable')
    case 'suspension_list_missing': return t('credentials.detail.trustPending.suspensionListMissing')
    case 'suspension_list_unavailable': return t('credentials.detail.trustPending.suspensionListUnavailable')
    case 'supersession_state_unavailable': return t('credentials.detail.trustPending.supersessionUnavailable')
    case 'integrity_anchor_unavailable': return t('credentials.detail.trustPending.anchorUnavailable')
  }
}

function mismatchLabel(reason: EndorsementMismatch): string {
  switch (reason) {
    case 'wrong_network': return t('credentials.detail.trustMismatch.wrongNetwork')
    case 'subject_mismatch': return t('credentials.detail.trustMismatch.subjectMismatch')
    case 'not_skill_claim': return t('credentials.detail.trustMismatch.notSkillClaim')
    case 'course_document_not_claimed': return t('credentials.detail.trustMismatch.documentNotClaimed')
    case 'completion_root_not_claimed': return t('credentials.detail.trustMismatch.rootNotClaimed')
  }
}

function endorsementLabel(outcome: EndorsementOutcome): string {
  switch (outcome.outcome) {
    case 'not_supplied': return t('credentials.detail.trustDetail.notSupplied')
    case 'not_applicable':
      return t('credentials.detail.trustDetail.notApplicable', { reason: mismatchLabel(outcome.reason) })
    case 'invalid_evidence': return t('credentials.detail.trustDetail.invalidEvidence')
    case 'threshold_unmet':
      return t('credentials.detail.trustDetail.thresholdUnmet', {
        valid: outcome.valid_attestors,
        required: outcome.required_attestors,
      })
  }
}

/** Shown only when no provenance can be stated: invalid or pending. */
const trustLabel = computed((): string => {
  switch (trust.value?.state) {
    case 'invalid': return t('credentials.detail.trustState.invalid')
    case 'pending': return t('credentials.detail.trustState.pending')
    default: return t('credentials.detail.notVerified')
  }
})

function platformLabel(platform: AttestationPlatform): string {
  return platform === 'ios'
    ? t('credentials.detail.trustPlatform.ios')
    : t('credentials.detail.trustPlatform.android')
}

/** The one plain-words provenance, as a headline. */
const provenanceLabel = computed((): string | null => {
  const provenance = view.value?.provenance
  if (!provenance) return null
  switch (provenance.kind) {
    case 'organisation':
      return t('credentials.detail.trustProvenance.organisation', { name: provenance.name, domain: provenance.domain })
    case 'device_attested': return t('credentials.detail.trustProvenance.deviceAttested')
    case 'self_reported': return t('credentials.detail.trustProvenance.selfReported')
  }
})

const provenanceVariant = computed(() => {
  switch (view.value?.provenance?.kind) {
    case 'organisation': return 'success'
    case 'device_attested': return 'primary'
    case 'self_reported': return 'secondary'
    default: return trust.value?.state === 'invalid' ? 'error' : 'warning'
  }
})

/** What the provenance means and, as plainly, what it does not. */
const provenanceLines = computed((): string[] => {
  const value = view.value
  const provenance = value?.provenance
  if (!value || !provenance) return []
  const lines: string[] = []
  switch (provenance.kind) {
    case 'organisation':
      lines.push(t('credentials.detail.trustProvenance.organisationBody', { name: provenance.name, domain: provenance.domain }))
      break
    case 'device_attested':
      lines.push(t('credentials.detail.trustProvenance.deviceAttestedBody', { platform: platformLabel(provenance.platform) }))
      lines.push(t('credentials.detail.trustProvenance.deviceAttestedLimits'))
      break
    case 'self_reported': {
      lines.push(t('credentials.detail.trustProvenance.selfReportedBody'))
      const attestation = provenance.attestation
      if (attestation.outcome === 'verified_session_bound') {
        lines.push(t('credentials.detail.trustProvenance.sessionBound', { platform: platformLabel(attestation.platform) }))
      } else if (attestation.outcome === 'verified_credential_bound' && attestation.companion) {
        lines.push(t('credentials.detail.trustProvenance.companion', { platform: platformLabel(attestation.platform) }))
      } else if (attestation.outcome === 'unverifiable') {
        lines.push(t('credentials.detail.trustProvenance.unverifiable', { platform: platformLabel(attestation.platform) }))
      }
      break
    }
  }
  if (value.issuer && provenance.kind !== 'organisation') {
    lines.push(t('credentials.detail.trustProvenance.signedBy', { issuer: value.issuer }))
  }
  switch (value.organisation?.outcome) {
    case 'key_not_listed':
      lines.push(t('credentials.detail.trustProvenance.keyNotListed'))
      break
    case 'wrong_domain':
    case 'invalid_descriptor':
    case 'descriptor_not_supplied':
      lines.push(t('credentials.detail.trustProvenance.organisationUnchecked'))
      break
  }
  const classified = value.trust
  if (classified.state === 'verified_course_endorsement') {
    lines.push(t('credentials.detail.trustDetail.courseEndorsement', {
      count: classified.attestors.length,
      version: classified.course_document_version,
    }))
  } else if (classified.state === 'verified_self_claim' && classified.endorsement.outcome !== 'not_supplied') {
    lines.push(endorsementLabel(classified.endorsement))
  }
  return lines
})

/** Why no provenance can be stated, for an invalid or pending credential. */
const trustDetail = computed((): string => {
  const value = trust.value
  if (!value || view.value?.provenance) return ''
  switch (value.state) {
    case 'invalid':
      return t('credentials.detail.trustDetail.invalid', {
        reasons: value.reasons.map(invalidReasonLabel).join(', '),
      })
    case 'pending':
      return t('credentials.detail.trustDetail.pending', {
        reasons: value.reasons.map(pendingReasonLabel).join(', '),
      })
    default: return ''
  }
})

const suspensionLabel = computed((): string => {
  switch (view.value?.suspension.state) {
    case 'suspended': return t('credentials.detail.trustSuspension.suspended')
    case 'unknown': return t('credentials.detail.trustSuspension.unknown')
    case 'no_list': return t('credentials.detail.trustSuspension.noList')
    default: return t('credentials.detail.trustSuspension.notSuspended')
  }
})

const suspensionVariant = computed(() => {
  switch (view.value?.suspension.state) {
    case 'suspended': return 'error'
    case 'unknown': return 'warning'
    case 'no_list': return 'secondary'
    default: return 'success'
  }
})

const suspensionDetail = computed((): string => {
  const suspension = view.value?.suspension
  switch (suspension?.state) {
    case 'suspended': return t('credentials.detail.trustSuspension.suspendedBody')
    case 'unknown':
      return t('credentials.detail.trustSuspension.unknownBody', { reason: pendingReasonLabel(suspension.reason) })
    case 'no_list': return t('credentials.detail.trustSuspension.noListBody')
    default: return ''
  }
})

function classOf(c: VerifiableCredential): string {
  return c.type.find((t) => t !== 'VerifiableCredential') ?? 'Credential'
}

function back() {
  router.push({ name: 'credentials' })
}

// A missing or unreadable status list means revocation is unknown, not "no".
const statusEvidencePending = computed(() =>
  (verification.value?.pendingReasons ?? []).some(
    (reason) => reason === 'status_list_missing' || reason === 'status_list_unavailable',
  ),
)

// Likewise a missing or unreadable suspension list: suspension is unknown.
const suspensionEvidencePending = computed(() =>
  (verification.value?.pendingReasons ?? []).some(
    (reason) => reason === 'suspension_list_missing' || reason === 'suspension_list_unavailable',
  ),
)

const decisionVariant = computed(() => {
  if (!verification.value) return 'secondary'
  if (verification.value.acceptanceDecision === 'accept') return 'success'
  return verification.value.acceptanceDecision === 'pending' ? 'warning' : 'error'
})
</script>

<template>
  <div>
    <button class="mb-4 text-sm text-muted-foreground hover:text-foreground" @click="back">
      ← {{ $t('credentials.detail.back') }}
    </button>

    <div v-if="loading" class="animate-pulse rounded-xl bg-card shadow-sm p-6 h-64" />

    <AppAlert v-else-if="error" variant="error" :message="error" />

    <template v-else-if="credential">
      <div class="mb-6 flex items-start justify-between gap-4">
        <div class="min-w-0">
          <h1
            class="text-2xl font-bold text-foreground truncate"
            :title="credential.id ?? $t('credentials.detail.noId')"
          >
            {{ credential.id ?? $t('credentials.detail.noId') }}
          </h1>
          <div class="mt-2 flex items-center gap-2">
            <AppBadge variant="primary">{{ classOf(credential) }}</AppBadge>
            <AppBadge :variant="decisionVariant">
              {{ verification?.acceptanceDecision ?? $t('credentials.detail.notVerified') }}
            </AppBadge>
          </div>
        </div>
        <div class="flex gap-2 flex-shrink-0">
          <AppButton variant="outline" :loading="verifying" @click="reverify">
            {{ $t('credentials.detail.recheck') }}
          </AppButton>
          <AppButton variant="danger" @click="showRevoke = true">{{ $t('credentials.detail.revoke') }}</AppButton>
        </div>
      </div>

      <AppAlert v-if="statusPublish" class="mb-6" :variant="statusPublish.variant" :message="statusPublish.message" />

      <!-- Provenance: what the trust command states, never a privilege grant -->
      <section v-if="view" class="mb-6 rounded-xl bg-card shadow-sm p-6" data-trust-section>
        <div class="mb-2 flex items-center justify-between gap-3">
          <h2 class="text-base font-semibold">{{ $t('credentials.detail.trustTitle') }}</h2>
          <AppBadge :variant="provenanceVariant">
            <span v-if="provenanceLabel" data-provenance>{{ provenanceLabel }}</span>
            <span v-else>{{ trustLabel }}</span>
          </AppBadge>
        </div>
        <p v-for="line in provenanceLines" :key="line" class="mt-1 break-words text-sm text-foreground">{{ line }}</p>
        <p v-if="trustDetail" class="break-all text-sm text-foreground">{{ trustDetail }}</p>
        <div class="mt-4 flex items-center justify-between gap-3 text-sm">
          <span class="text-muted-foreground">{{ $t('credentials.detail.trustSuspension.title') }}</span>
          <AppBadge :variant="suspensionVariant"><span data-suspension>{{ suspensionLabel }}</span></AppBadge>
        </div>
        <p v-if="suspensionDetail" class="mt-1 text-xs text-muted-foreground">{{ suspensionDetail }}</p>
        <p v-if="secondDevice" class="mt-4 text-sm font-medium text-foreground" data-second-device>{{ secondDevice }}</p>
        <details v-if="credential.integrity?.coverageStatementDigest" class="mt-4 text-sm" open>
          <summary class="cursor-pointer text-muted-foreground">{{ $t('sentinel.trust.coverage.title') }}</summary>
          <CoverageStatementView class="mt-2" :statement="view.coverage_statement" :digest="credential.integrity.coverageStatementDigest" />
        </details>
        <p class="mt-3 text-xs text-muted-foreground">{{ $t('credentials.detail.trustNote') }}</p>
      </section>

      <!-- Verification result panel -->
      <section v-if="verification" class="mb-6 rounded-xl bg-card shadow-sm p-6">
        <h2 class="text-base font-semibold mb-3">{{ $t('credentials.detail.proofTitle') }}</h2>
        <dl class="grid grid-cols-2 gap-3 sm:grid-cols-3 text-sm">
          <div>
            <dt class="text-xs text-muted-foreground">{{ $t('credentials.detail.signature') }}</dt>
            <dd>
              <AppBadge :variant="verification.validSignature ? 'success' : 'error'">
                {{ verification.validSignature ? $t('credentials.value.signed') : $t('credentials.value.notSigned') }}
              </AppBadge>
            </dd>
          </div>
          <div>
            <dt class="text-xs text-muted-foreground">{{ $t('credentials.detail.issuerResolved') }}</dt>
            <dd>
              <AppBadge :variant="verification.issuerResolved ? 'success' : 'error'">
                {{ verification.issuerResolved ? $t('credentials.value.yes') : $t('credentials.value.no') }}
              </AppBadge>
            </dd>
          </div>
          <div>
            <dt class="text-xs text-muted-foreground">{{ $t('credentials.detail.subjectBound') }}</dt>
            <dd>
              <AppBadge :variant="verification.subjectBound ? 'success' : 'error'">
                {{ verification.subjectBound ? $t('credentials.value.yes') : $t('credentials.value.no') }}
              </AppBadge>
            </dd>
          </div>
          <div>
            <dt class="text-xs text-muted-foreground">{{ $t('credentials.detail.revoked') }}</dt>
            <dd>
              <AppBadge v-if="statusEvidencePending && !verification.revoked" variant="warning">
                {{ $t('credentials.value.pending') }}
              </AppBadge>
              <AppBadge v-else :variant="verification.revoked ? 'error' : 'success'">
                {{ verification.revoked ? $t('credentials.value.yes') : $t('credentials.value.no') }}
              </AppBadge>
            </dd>
          </div>
          <div data-suspended-check>
            <dt class="text-xs text-muted-foreground">{{ $t('credentials.detail.suspended') }}</dt>
            <dd>
              <AppBadge v-if="suspensionEvidencePending && !verification.suspended" variant="warning">
                {{ $t('credentials.value.pending') }}
              </AppBadge>
              <AppBadge v-else :variant="verification.suspended ? 'error' : 'success'">
                {{ verification.suspended ? $t('credentials.value.yes') : $t('credentials.value.no') }}
              </AppBadge>
            </dd>
          </div>
          <div>
            <dt class="text-xs text-muted-foreground">{{ $t('credentials.detail.expired') }}</dt>
            <dd>
              <AppBadge :variant="verification.expired ? 'warning' : 'success'">
                {{ verification.expired ? $t('credentials.value.yes') : $t('credentials.value.no') }}
              </AppBadge>
            </dd>
          </div>
          <div>
            <dt class="text-xs text-muted-foreground">{{ $t('credentials.detail.tamperProof') }}</dt>
            <dd>
              <AppBadge :variant="verification.integrityAnchored ? 'success' : 'secondary'">
                {{ verification.integrityAnchored ? $t('credentials.value.yes') : $t('credentials.value.pending') }}
              </AppBadge>
            </dd>
          </div>
        </dl>
        <p class="mt-3 text-xs text-muted-foreground">
          {{ $t('credentials.detail.verifiedAt', { time: verification.verificationTime }) }}
        </p>
        <p class="mt-1 text-xs text-muted-foreground">
          {{ $t('credentials.detail.statusEvidenceNote') }}
        </p>
      </section>

      <!-- Cardano integrity anchor (§11.3.1) -->
      <section class="mb-6 rounded-xl bg-card shadow-sm p-6" data-anchor-section>
        <div class="mb-2 flex items-center justify-between gap-3">
          <h2 class="text-base font-semibold">{{ $t('credentials.anchor.title') }}</h2>
          <AppBadge v-if="anchor" :variant="anchorBadge(anchor.status).variant">
            {{ $t(anchorBadge(anchor.status).label) }}
          </AppBadge>
        </div>
        <p class="break-words text-sm text-foreground">{{ anchorBody }}</p>
        <p v-if="anchor?.needs_funds" class="mt-2 text-sm text-muted-foreground">
          {{ $t('credentials.anchor.body.failedHint') }}
        </p>
        <div v-if="anchor?.tx_hash || anchor?.status === 'failed'" class="mt-3 flex flex-wrap items-center gap-3 text-sm">
          <a
            v-if="anchor?.tx_hash"
            class="text-primary hover:underline"
            :href="explorerTxUrl(anchor.tx_hash)"
            target="_blank"
            rel="noopener"
          >{{ $t('credentials.anchor.viewTransaction') }}</a>
          <span v-if="anchor?.tx_hash" class="truncate font-mono text-xs text-muted-foreground" :title="anchor.tx_hash">
            {{ anchor.tx_hash }}
          </span>
          <AppButton
            v-if="anchor?.status === 'failed'"
            variant="outline"
            size="sm"
            :loading="retryingAnchor"
            @click="retryAnchor"
          >
            {{ $t('credentials.anchor.retry') }}
          </AppButton>
        </div>
        <p class="mt-3 text-xs text-muted-foreground">{{ $t('credentials.anchor.note') }}</p>
      </section>

      <!-- Integrity attestation (§ Integrity→VC bridge) -->
      <section v-if="credential.integrity" class="mb-6 rounded-xl bg-card shadow-sm p-6" data-integrity-section>
        <h2 class="text-base font-semibold mb-3">{{ $t('credentials.detail.integrityTitle') }}</h2>
        <div class="grid gap-2 text-sm">
          <div class="flex items-center justify-between">
            <span class="text-muted-foreground">{{ $t('credentials.detail.assurance') }}</span>
            <AppBadge :variant="credential.integrity.assuranceLevel === 'high_assurance' ? 'success' : (credential.integrity.assuranceLevel === 'anchored' || credential.integrity.assuranceLevel === 'device_attested') ? 'accent' : 'secondary'">
              {{ credential.integrity.assuranceLevel }}
            </AppBadge>
          </div>
          <div class="flex items-center justify-between">
            <span class="text-muted-foreground">{{ $t('credentials.detail.status') }}</span>
            <span class="text-foreground">{{ credential.integrity.status }}</span>
          </div>
          <div class="flex items-center justify-between">
            <span class="text-muted-foreground">{{ $t('credentials.detail.integrityScore') }}</span>
            <span class="text-foreground">{{ credential.integrity.integrityScore ?? $t('credentials.detail.na') }}</span>
          </div>
          <div class="flex items-center justify-between">
            <span class="text-muted-foreground">{{ $t('credentials.detail.flags') }}</span>
            <span class="text-foreground">{{ $t('credentials.detail.flagsValue', { critical: credential.integrity.criticalCount, warning: credential.integrity.warningCount }) }}</span>
          </div>
          <div v-if="credential.integrity.trustProfile" class="flex items-center justify-between">
            <span class="text-muted-foreground">{{ $t('credentials.detail.integrityProfile') }}</span>
            <span class="text-foreground">{{ $t(`sentinel.trust.profile.${credential.integrity.trustProfile}`) }}</span>
          </div>
          <div class="flex items-center justify-between" data-answer-custody>
            <span class="text-muted-foreground">{{ $t('credentials.detail.custodyLabel') }}</span>
            <span class="text-foreground">{{ $t(`credentials.detail.custody.${credential.integrity.answerCustody ?? 'device'}`) }}</span>
          </div>
          <div v-if="grader" class="flex items-center justify-between" data-graded-by>
            <span class="text-muted-foreground">{{ $t('credentials.detail.gradedByLabel') }}</span>
            <span class="text-foreground">{{ $t(`credentials.detail.gradedBy.${grader.by}`) }}</span>
          </div>
          <div v-if="grader?.level" class="flex items-center justify-between" data-issuer-level>
            <span class="text-muted-foreground">{{ $t('credentials.detail.issuerLevelLabel') }}</span>
            <span class="text-foreground">{{ $t(`credentials.detail.assuranceLevel.${grader.level}`) }}</span>
          </div>
          <div v-if="credential.integrity.verificationRung" class="flex items-center justify-between">
            <span class="text-muted-foreground">{{ $t('credentials.detail.integrityRung') }}</span>
            <span class="text-foreground">{{ $t(`sentinel.trust.rung.${credential.integrity.verificationRung}`) }}</span>
          </div>
          <div v-if="credential.integrity.signalsRun?.length" class="flex items-start justify-between gap-3">
            <span class="text-muted-foreground">{{ $t('credentials.detail.signalsRun') }}</span>
            <span class="text-end text-foreground">{{ signalNames(credential.integrity.signalsRun) }}</span>
          </div>
          <div v-if="credential.integrity.signalsDegraded?.length" class="flex items-start justify-between gap-3">
            <span class="text-muted-foreground">{{ $t('credentials.detail.signalsDegraded') }}</span>
            <span class="text-end text-warning">{{ signalNames(credential.integrity.signalsDegraded) }}</span>
          </div>
          <div v-if="credential.integrity.cameraCoverage != null" class="flex items-center justify-between" data-camera-coverage>
            <span class="text-muted-foreground">{{ $t('credentials.detail.cameraCoverage') }}</span>
            <span :class="credential.integrity.cameraCoverage < 1 ? 'text-warning' : 'text-foreground'">{{ $t('credentials.detail.cameraCoverageValue', { percent: coveragePercent(credential.integrity.cameraCoverage) }) }}</span>
          </div>
          <div v-if="credential.integrity.signalsNotRun?.length" class="flex items-start justify-between gap-3">
            <span class="text-muted-foreground">{{ $t('credentials.detail.signalsNotRun') }}</span>
            <span class="text-end text-muted-foreground">{{ signalNames(credential.integrity.signalsNotRun) }}</span>
          </div>
          <div v-if="credential.integrity.probe" class="flex items-center justify-between">
            <span class="text-muted-foreground">{{ $t('credentials.detail.probe') }}</span>
            <span class="text-foreground">{{ $t('credentials.detail.probeValue', {
              satisfied: credential.integrity.probe.satisfied,
              failed: credential.integrity.probe.failed,
              skipped: credential.integrity.probe.skipped,
            }) }}</span>
          </div>
          <details v-if="credential.integrity.commitmentRoot || credential.integrity.anchorRef">
            <summary class="cursor-pointer text-muted-foreground">{{ $t('common.advanced.toggle') }}</summary>
            <div class="mt-2 grid gap-2">
              <div v-if="credential.integrity.commitmentRoot" class="flex items-center justify-between gap-3">
                <span class="text-muted-foreground">{{ $t('credentials.detail.commitmentRoot') }}</span>
                <span class="truncate font-mono text-xs text-muted-foreground">{{ credential.integrity.commitmentRoot }}</span>
              </div>
              <div v-if="credential.integrity.anchorRef" class="flex items-center justify-between gap-3">
                <span class="text-muted-foreground">{{ $t('credentials.detail.publicRecord') }}</span>
                <span class="truncate font-mono text-xs text-muted-foreground">{{ credential.integrity.anchorRef }}</span>
              </div>
            </div>
          </details>
        </div>
        <p class="mt-3 text-xs text-muted-foreground">
          {{ $t('credentials.detail.integrityNote') }}
          <span v-if="credential.integrity.assuranceLevel === 'local'">{{ $t('credentials.detail.integrityLocal') }}</span>
          <span v-else-if="credential.integrity.assuranceLevel === 'device_attested'">{{ $t('credentials.detail.integrityDeviceAttested') }}</span>
          <span v-else-if="credential.integrity.assuranceLevel === 'companion_attested'">{{ $t('credentials.detail.integrityCompanionAttested') }}</span>
        </p>
      </section>

      <!-- Raw payload -->
      <section class="rounded-xl bg-card shadow-sm p-6">
        <details>
          <summary class="cursor-pointer text-base font-semibold">{{ $t('credentials.detail.fullDetails') }}</summary>
          <pre class="mt-3 max-h-96 overflow-auto rounded-md bg-muted/30 p-3 text-xs font-mono">{{ JSON.stringify(credential, null, 2) }}</pre>
        </details>
      </section>

      <!-- Revoke confirm -->
      <div
        v-if="showRevoke"
        class="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4"
        @click.self="showRevoke = false"
      >
        <div class="card p-6 w-full max-w-md">
          <h2 class="text-base font-semibold mb-2">{{ $t('credentials.detail.revokeTitle') }}</h2>
          <p class="text-sm text-muted-foreground mb-4">
            {{ $t('credentials.detail.revokeBody') }}
          </p>
          <input
            v-model="revokeReason"
            class="input mb-4"
            :placeholder="$t('credentials.detail.revokeReason')"
          />
          <div class="flex justify-end gap-2">
            <AppButton variant="ghost" @click="showRevoke = false">{{ $t('common.actions.cancel') }}</AppButton>
            <AppButton variant="danger" :loading="revoking" @click="revoke">
              {{ $t('credentials.detail.revoke') }}
            </AppButton>
          </div>
        </div>
      </div>
    </template>
  </div>
</template>
