<script setup lang="ts">
import { ref, onMounted, computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRoute, useRouter } from 'vue-router'
import { AppBadge, AppButton, AppAlert } from '@/components/ui'
import { useCredentials } from '@/composables/useCredentials'
import type { VerifiableCredential, VerificationResult } from '@/types'

const { t } = useI18n()
const route = useRoute()
const router = useRouter()
const api = useCredentials()

const credentialId = computed(() => route.params.id as string)

const credential = ref<VerifiableCredential | null>(null)
const verification = ref<VerificationResult | null>(null)
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
  } else {
    error.value = api.error.value ?? t('credentials.detail.notFound')
  }
  loading.value = false
})

async function reverify() {
  if (!credential.value) return
  verifying.value = true
  verification.value = (await api.verify(credential.value)) ?? null
  verifying.value = false
}

async function revoke() {
  if (!credential.value?.id) return
  revoking.value = true
  await api.revoke(credential.value.id, revokeReason.value || t('credentials.detail.revokeDefaultReason'))
  revoking.value = false
  showRevoke.value = false
  // Re-verify to surface the revoked flag.
  verification.value = (await api.verify(credential.value)) ?? null
}

function classOf(c: VerifiableCredential): string {
  return c.type.find((t) => t !== 'VerifiableCredential') ?? 'Credential'
}

function back() {
  router.push({ name: 'credentials' })
}

const decisionVariant = computed(() => {
  if (!verification.value) return 'secondary'
  return verification.value.acceptance_decision === 'accept' ? 'success' : 'error'
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
              {{ verification?.acceptance_decision ?? $t('credentials.detail.notVerified') }}
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

      <!-- Verification result panel -->
      <section v-if="verification" class="mb-6 rounded-xl bg-card shadow-sm p-6">
        <h2 class="text-base font-semibold mb-3">{{ $t('credentials.detail.proofTitle') }}</h2>
        <dl class="grid grid-cols-2 gap-3 sm:grid-cols-3 text-sm">
          <div>
            <dt class="text-xs text-muted-foreground">{{ $t('credentials.detail.signature') }}</dt>
            <dd>
              <AppBadge :variant="verification.valid_signature ? 'success' : 'error'">
                {{ verification.valid_signature ? $t('credentials.value.signed') : $t('credentials.value.notSigned') }}
              </AppBadge>
            </dd>
          </div>
          <div>
            <dt class="text-xs text-muted-foreground">{{ $t('credentials.detail.issuerResolved') }}</dt>
            <dd>
              <AppBadge :variant="verification.issuer_resolved ? 'success' : 'error'">
                {{ verification.issuer_resolved ? $t('credentials.value.yes') : $t('credentials.value.no') }}
              </AppBadge>
            </dd>
          </div>
          <div>
            <dt class="text-xs text-muted-foreground">{{ $t('credentials.detail.subjectBound') }}</dt>
            <dd>
              <AppBadge :variant="verification.subject_bound ? 'success' : 'error'">
                {{ verification.subject_bound ? $t('credentials.value.yes') : $t('credentials.value.no') }}
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
              <AppBadge :variant="verification.integrity_anchored ? 'success' : 'secondary'">
                {{ verification.integrity_anchored ? $t('credentials.value.yes') : $t('credentials.value.pending') }}
              </AppBadge>
            </dd>
          </div>
        </dl>
        <p class="mt-3 text-xs text-muted-foreground">
          {{ $t('credentials.detail.verifiedAt', { time: verification.verification_time }) }}
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
