<script setup lang="ts">
import { onMounted, onUnmounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { useAccountStatus } from '@/composables/useAccountStatus'
import { useProfiles } from '@/composables/useProfiles'
import { useLocalApi } from '@/composables/useLocalApi'
import { useP2P } from '@/composables/useP2P'
import Starfield from '@/components/auth/Starfield.vue'

const router = useRouter()
const { refreshAccountStatus } = useAccountStatus()
const { lockProfile } = useProfiles()
const { invoke } = useLocalApi()
// The gate uses the blank layout, so the app shell (which normally
// brings the P2P node online) never mounts here. Guardian invite
// generation + the link handshake both need the node running, so start
// it explicitly.
const { start: startP2P, startPolling } = useP2P()

const checking = ref(false)
const inviteCode = ref('')
const inviteError = ref('')
const generating = ref(false)
const copied = ref(false)

let pollTimer: ReturnType<typeof setInterval> | null = null

onMounted(async () => {
  void ensureStillGated()
  // Bring the P2P node online before minting an invite, then poll its
  // status so the "network still starting" hint clears on its own.
  startPolling(2000)
  try {
    await startP2P()
  } catch {
    // Non-fatal — generateInvite surfaces its own error and can retry.
  }
  void generateInvite()
  // The link handshake happens in the background the moment the parent
  // accepts — poll so the gate lifts without a manual refresh.
  pollTimer = setInterval(() => void ensureStillGated(true), 5000)
})

onUnmounted(() => {
  if (pollTimer) clearInterval(pollTimer)
})

async function ensureStillGated(silent = false) {
  if (!silent) checking.value = true
  try {
    const status = await refreshAccountStatus()
    if (!status || status.activation_state !== 'pending_guardian') {
      router.replace('/home')
    }
  } finally {
    if (!silent) checking.value = false
  }
}

async function generateInvite(retries = 8) {
  generating.value = true
  inviteError.value = ''
  try {
    inviteCode.value = await invoke<string>('guardian_create_invite')
  } catch (e) {
    const msg = String(e)
    // The node needs a moment to acquire a PeerId + dial addresses on a
    // cold start. Retry silently a few times before surfacing the error.
    if (retries > 0 && /still starting|PeerId|no PeerId/i.test(msg)) {
      setTimeout(() => void generateInvite(retries - 1), 1500)
      return
    }
    inviteError.value = msg
  } finally {
    generating.value = false
  }
}

async function copyInvite() {
  await navigator.clipboard.writeText(inviteCode.value)
  copied.value = true
  setTimeout(() => { copied.value = false }, 2000)
}

async function backToProfiles() {
  await lockProfile()
  router.replace('/profiles')
}
</script>

<template>
  <div class="min-h-full bg-background relative overflow-y-auto flex items-center justify-center p-4 sm:p-6 lg:p-8">
    <Starfield />

    <div class="w-full max-w-xl relative z-10">
      <div class="rounded-2xl border border-border/70 bg-card/80 backdrop-blur px-6 py-8 text-center">
        <div class="w-16 h-16 rounded-full bg-warning/10 flex items-center justify-center mx-auto mb-4">
          <svg class="w-8 h-8 text-warning" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
            <path stroke-linecap="round" stroke-linejoin="round" d="M16.5 10.5V6.75a4.5 4.5 0 10-9 0v3.75m-.75 11.25h10.5a2.25 2.25 0 002.25-2.25v-6.75a2.25 2.25 0 00-2.25-2.25H6.75a2.25 2.25 0 00-2.25 2.25v6.75a2.25 2.25 0 002.25 2.25z" />
          </svg>
        </div>

        <h1 class="text-2xl font-bold mb-2 text-foreground">{{ $t('onboarding.guardianGate.heading') }}</h1>
        <p class="text-sm text-muted-foreground mb-6">
          {{ $t('onboarding.guardianGate.subtitle') }}
        </p>

        <div class="card p-5 mb-5 text-start">
          <h2 class="text-sm font-semibold text-foreground mb-2">{{ $t('onboarding.guardianGate.howHeading') }}</h2>
          <ul class="space-y-2 text-sm text-muted-foreground">
            <li class="flex items-start gap-2">
              <span class="text-primary mt-0.5 font-mono text-xs w-4 text-end shrink-0">01</span>
              {{ $t('onboarding.guardianGate.how1') }}
            </li>
            <li class="flex items-start gap-2">
              <span class="text-primary mt-0.5 font-mono text-xs w-4 text-end shrink-0">02</span>
              {{ $t('onboarding.guardianGate.how2') }}
            </li>
            <li class="flex items-start gap-2">
              <span class="text-primary mt-0.5 font-mono text-xs w-4 text-end shrink-0">03</span>
              {{ $t('onboarding.guardianGate.how3') }}
            </li>
          </ul>
        </div>

        <!-- Invite code -->
        <div class="card p-5 mb-5 text-start">
          <div class="flex items-center justify-between mb-2">
            <h2 class="text-sm font-semibold text-foreground">{{ $t('onboarding.guardianGate.codeHeading') }}</h2>
            <button
              class="text-xs text-primary hover:underline disabled:opacity-50"
              :disabled="generating"
              @click="() => generateInvite()"
            >
              {{ generating ? $t('onboarding.guardianGate.generating') : $t('onboarding.guardianGate.newCode') }}
            </button>
          </div>
          <p class="mb-2 text-xs text-muted-foreground">
            {{ $t('onboarding.guardianGate.codeHint') }}
          </p>
          <div v-if="inviteCode" class="space-y-2">
            <code class="block max-h-24 overflow-y-auto break-all rounded-lg bg-muted/30 p-3 font-mono text-xs text-foreground">{{ inviteCode }}</code>
            <button
              class="w-full flex items-center justify-center gap-2 py-2 px-3 rounded-md text-xs font-medium border border-border transition-colors"
              :class="copied ? 'bg-success/10 text-success border-success/30' : 'text-muted-foreground hover:bg-muted/50 hover:text-foreground'"
              @click="copyInvite"
            >
              {{ copied ? $t('onboarding.guardianGate.copied') : $t('onboarding.guardianGate.copyCode') }}
            </button>
          </div>
          <p v-else-if="inviteError" class="text-sm text-error">{{ inviteError }}</p>
          <div v-else class="h-16 animate-pulse rounded-lg bg-muted/30" />
        </div>

        <button
          class="w-full py-2.5 px-4 rounded-md text-sm font-medium bg-primary text-primary-foreground hover:bg-primary-hover transition-colors disabled:opacity-50"
          :disabled="checking"
          @click="ensureStillGated()"
        >
          {{ checking ? $t('onboarding.guardianGate.checking') : $t('onboarding.guardianGate.checkActivation') }}
        </button>

        <button
          class="w-full mt-3 py-2 text-sm text-muted-foreground hover:text-foreground transition-colors"
          @click="backToProfiles"
        >
          {{ $t('onboarding.guardianGate.backToProfiles') }}
        </button>
      </div>
    </div>
  </div>
</template>
