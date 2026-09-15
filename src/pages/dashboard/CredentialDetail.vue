<script setup lang="ts">
import { ref, onMounted, computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRoute, useRouter } from 'vue-router'
import { AppBadge, AppButton, AppAlert } from '@/components/ui'
import { useCredentials } from '@/composables/useCredentials'
import type {
  CredentialTrust,
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
const trust = ref<CredentialTrust | null>(null)
const loading = ref(true)
const verifying = ref(false)
const revoking = ref(false)
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

async function reverify() {
  if (!credential.value) return
  verifying.value = true
  verification.value = (await api.verify(credential.value)) ?? null
  await loadTrust()
  verifying.value = false
}

async function loadTrust() {
  trust.value = (await api.trust(credentialId.value)) ?? null
}

async function revoke() {
  if (!credential.value?.id) return
  revoking.value = true
  await api.revoke(credential.value.id, revokeReason.value || t('credentials.detail.revokeDefaultReason'))
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
  }
}

function pendingReasonLabel(reason: VerificationPendingReason): string {
  switch (reason) {
    case 'issuer_key_missing': return t('credentials.detail.trustPending.issuerKeyMissing')
    case 'issuer_key_unavailable': return t('credentials.detail.trustPending.issuerKeyUnavailable')
    case 'status_list_missing': return t('credentials.detail.trustPending.statusListMissing')
    case 'status_list_unavailable': return t('credentials.detail.trustPending.statusListUnavailable')
    case 'suspension_state_unavailable': return t('credentials.detail.trustPending.suspensionUnavailable')
    case 'supersession_state_unavailable': return t('credentials.detail.trustPending.supersessionUnavailable')
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

const trustLabel = computed((): string => {
  switch (trust.value?.state) {
    case 'invalid': return t('credentials.detail.trustState.invalid')
    case 'pending': return t('credentials.detail.trustState.pending')
    case 'verified_self_claim': return t('credentials.detail.trustState.selfClaim')
    case 'verified_issuer_signed': return t('credentials.detail.trustState.issuerSigned')
    case 'verified_course_endorsement': return t('credentials.detail.trustState.courseEndorsement')
    default: return t('credentials.detail.notVerified')
  }
})

const trustVariant = computed(() => {
  switch (trust.value?.state) {
    case 'invalid': return 'error'
    case 'pending': return 'warning'
    case 'verified_course_endorsement': return 'success'
    case 'verified_issuer_signed': return 'primary'
    default: return 'secondary'
  }
})

const trustDetail = computed((): string => {
  const value = trust.value
  if (!value) return ''
  switch (value.state) {
    case 'invalid':
      return t('credentials.detail.trustDetail.invalid', {
        reasons: value.reasons.map(invalidReasonLabel).join(', '),
      })
    case 'pending':
      return t('credentials.detail.trustDetail.pending', {
        reasons: value.reasons.map(pendingReasonLabel).join(', '),
      })
    case 'verified_self_claim': return endorsementLabel(value.endorsement)
    case 'verified_issuer_signed':
      return t('credentials.detail.trustDetail.issuerSigned', { issuer: value.issuer })
    case 'verified_course_endorsement':
      return t('credentials.detail.trustDetail.courseEndorsement', {
        count: value.attestors.length,
        version: value.course_document_version,
      })
  }
})

function classOf(c: VerifiableCredential): string {
  return c.type.find((t) => t !== 'VerifiableCredential') ?? 'Credential'
}

function back() {
  router.push({ name: 'credentials' })
}

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

      <!-- Provenance: typed trust classification, never a privilege grant -->
      <section v-if="trust" class="mb-6 rounded-xl bg-card shadow-sm p-6">
        <div class="mb-2 flex items-center justify-between gap-3">
          <h2 class="text-base font-semibold">{{ $t('credentials.detail.trustTitle') }}</h2>
          <AppBadge :variant="trustVariant">{{ trustLabel }}</AppBadge>
        </div>
        <p class="break-all text-sm text-foreground">{{ trustDetail }}</p>
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
              <AppBadge :variant="verification.revoked ? 'error' : 'success'">
                {{ verification.revoked ? $t('credentials.value.yes') : $t('credentials.value.no') }}
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
      </section>

      <!-- Integrity attestation (§ Integrity→VC bridge) -->
      <section v-if="credential.integrity" class="mb-6 rounded-xl bg-card shadow-sm p-6">
        <h2 class="text-base font-semibold mb-3">{{ $t('credentials.detail.integrityTitle') }}</h2>
        <div class="grid gap-2 text-sm">
          <div class="flex items-center justify-between">
            <span class="text-muted-foreground">{{ $t('credentials.detail.assurance') }}</span>
            <AppBadge :variant="credential.integrity.assuranceLevel === 'high_assurance' ? 'success' : credential.integrity.assuranceLevel === 'anchored' ? 'accent' : 'secondary'">
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
