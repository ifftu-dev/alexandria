<script setup lang="ts">
import { ref, onMounted, computed } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { AppBadge, AppButton, AppAlert } from '@/components/ui'
import { useCredentials } from '@/composables/useCredentials'
import type { VerifiableCredential, VerificationResult } from '@/types'

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
    error.value = api.error.value ?? `Credential ${credentialId.value} not found`
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
  if (!credential.value) return
  revoking.value = true
  await api.revoke(credential.value.id, revokeReason.value || 'no reason given')
  revoking.value = false
  showRevoke.value = false
  // Re-verify to surface the revoked flag.
  verification.value = (await api.verify(credential.value)) ?? null
}

function classOf(c: VerifiableCredential): string {
  return c.type.find((t) => t !== 'VerifiableCredential') ?? 'Credential'
}

function back() {
  router.push({ name: 'dashboard-credentials' })
}

const decisionVariant = computed(() => {
  if (!verification.value) return 'secondary'
  return verification.value.acceptance_decision === 'accept' ? 'success' : 'error'
})
</script>

<template>
  <div>
    <button class="mb-4 text-sm text-muted-foreground hover:text-foreground" @click="back">
      ← Back to credentials
    </button>

    <div v-if="loading" class="animate-pulse rounded-xl bg-card shadow-sm p-6 h-64" />

    <AppAlert v-else-if="error" variant="error" :message="error" />

    <template v-else-if="credential">
      <div class="mb-6 flex items-start justify-between gap-4">
        <div class="min-w-0">
          <h1 class="text-2xl font-bold text-foreground truncate" :title="credential.id">
            {{ credential.id }}
          </h1>
          <div class="mt-2 flex items-center gap-2">
            <AppBadge variant="primary">{{ classOf(credential) }}</AppBadge>
            <AppBadge :variant="decisionVariant">
              {{ verification?.acceptance_decision ?? 'not yet verified' }}
            </AppBadge>
          </div>
        </div>
        <div class="flex gap-2 flex-shrink-0">
          <AppButton variant="outline" :loading="verifying" @click="reverify">
            Re-verify
          </AppButton>
          <AppButton variant="danger" @click="showRevoke = true">Revoke</AppButton>
        </div>
      </div>

      <!-- Verification result panel -->
      <section v-if="verification" class="mb-6 rounded-xl bg-card shadow-sm p-6">
        <h2 class="text-base font-semibold mb-3">Verification</h2>
        <dl class="grid grid-cols-2 gap-3 sm:grid-cols-3 text-sm">
          <div>
            <dt class="text-xs text-muted-foreground">Signature</dt>
            <dd>
              <AppBadge :variant="verification.valid_signature ? 'success' : 'error'">
                {{ verification.valid_signature ? 'valid' : 'invalid' }}
              </AppBadge>
            </dd>
          </div>
          <div>
            <dt class="text-xs text-muted-foreground">Issuer resolved</dt>
            <dd>
              <AppBadge :variant="verification.issuer_resolved ? 'success' : 'error'">
                {{ verification.issuer_resolved ? 'yes' : 'no' }}
              </AppBadge>
            </dd>
          </div>
          <div>
            <dt class="text-xs text-muted-foreground">Subject bound</dt>
            <dd>
              <AppBadge :variant="verification.subject_bound ? 'success' : 'error'">
                {{ verification.subject_bound ? 'yes' : 'no' }}
              </AppBadge>
            </dd>
          </div>
          <div>
            <dt class="text-xs text-muted-foreground">Revoked</dt>
            <dd>
              <AppBadge :variant="verification.revoked ? 'error' : 'success'">
                {{ verification.revoked ? 'yes' : 'no' }}
              </AppBadge>
            </dd>
          </div>
          <div>
            <dt class="text-xs text-muted-foreground">Expired</dt>
            <dd>
              <AppBadge :variant="verification.expired ? 'warning' : 'success'">
                {{ verification.expired ? 'yes' : 'no' }}
              </AppBadge>
            </dd>
          </div>
          <div>
            <dt class="text-xs text-muted-foreground">Integrity anchored</dt>
            <dd>
              <AppBadge :variant="verification.integrity_anchored ? 'success' : 'secondary'">
                {{ verification.integrity_anchored ? 'yes' : 'pending' }}
              </AppBadge>
            </dd>
          </div>
        </dl>
        <p class="mt-3 text-xs text-muted-foreground">
          Verified at {{ verification.verification_time }}
        </p>
      </section>

      <!-- Raw payload -->
      <section class="rounded-xl bg-card shadow-sm p-6">
        <h2 class="text-base font-semibold mb-3">Raw credential</h2>
        <pre class="max-h-96 overflow-auto rounded-md bg-muted/30 p-3 text-xs font-mono">{{ JSON.stringify(credential, null, 2) }}</pre>
      </section>

      <!-- Revoke confirm -->
      <div
        v-if="showRevoke"
        class="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4"
        @click.self="showRevoke = false"
      >
        <div class="card p-6 w-full max-w-md">
          <h2 class="text-base font-semibold mb-2">Revoke credential</h2>
          <p class="text-sm text-muted-foreground mb-4">
            This flips the revocation bit in this issuer's status list.
            Verifiers across the network will reject the credential once
            the status list propagates.
          </p>
          <input
            v-model="revokeReason"
            class="input mb-4"
            placeholder="Reason (optional)"
          />
          <div class="flex justify-end gap-2">
            <AppButton variant="ghost" @click="showRevoke = false">Cancel</AppButton>
            <AppButton variant="danger" :loading="revoking" @click="revoke">
              Revoke
            </AppButton>
          </div>
        </div>
      </div>
    </template>
  </div>
</template>
