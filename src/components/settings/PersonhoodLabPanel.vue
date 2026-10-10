<script setup lang="ts">
import { computed, onMounted, onUnmounted } from 'vue'
import { usePersonhoodLab } from '@/composables/usePersonhoodLab'
import { usePersonhoodReceipts } from '@/composables/usePersonhoodReceipts'
import { useProfiles, onProfileLocked } from '@/composables/useProfiles'
import { AppAlert, AppButton } from '@/components/ui'

const lab = usePersonhoodLab()
const privateReceipts = usePersonhoodReceipts()
const { isUnlocked } = useProfiles()
const { status, busy, pending, error } = lab
const labels: Record<string, string> = {
  idle: 'Ready', downloading: 'Downloading test key', checking: 'Checking key integrity',
  witness: 'Preparing synthetic proof', proving: 'Generating proof', verifying: 'Verifying proof',
  cancelling: 'Stopping', cancelled: 'Cancelled — you can retry', complete: 'Complete', error: 'Unable to finish',
}
const phase = computed(() => labels[status.value?.phase ?? 'idle'] ?? 'Ready')
const percentage = computed(() => status.value ? Math.min(100,
  100 * status.value.downloaded_bytes / status.value.total_bytes) : 0)
let mounted = false
let timer: ReturnType<typeof setTimeout> | undefined
let stopProfileHook: (() => void) | undefined

async function poll() {
  await lab.refresh()
  if (mounted && status.value?.enabled) timer = setTimeout(() => void poll(), 400)
}

function visibilityChanged() {
  if (document.hidden) { void privateReceipts.cancel(); void lab.cancel() }
}

onMounted(() => {
  mounted = true
  document.addEventListener('visibilitychange', visibilityChanged)
  stopProfileHook = onProfileLocked(async () => { await privateReceipts.cancel(true); await lab.cancel() })
  void poll().then(() => { if (mounted && status.value?.enabled && isUnlocked.value) void privateReceipts.refresh() })
})

onUnmounted(() => {
  mounted = false
  clearTimeout(timer)
  stopProfileHook?.()
  document.removeEventListener('visibilitychange', visibilityChanged)
  void privateReceipts.cancel(true)
  void lab.cancel()
})
</script>

<template>
  <section v-if="status?.enabled" class="card space-y-4 p-4" data-testid="personhood-lab">
    <h3 class="text-base font-semibold text-foreground">Personhood Lab · developer preview</h3>
    <p class="text-sm text-muted-foreground">
      Runs a synthetic proof on this device. No identity documents are read, no credential is issued,
      and account permissions stay unchanged. Keep the app and this panel visible during the test.
    </p>
    <AppAlert variant="warning">
      The optional test key is a 612 MB download. Proving used about 800 MiB of memory on the test phone.
      Downloads can be cancelled and resumed. The key is checked before every proof.
    </AppAlert>
    <div class="space-y-2" aria-live="polite">
      <p class="font-medium text-foreground">{{ phase }}</p>
      <p class="text-sm text-muted-foreground">
        {{ (status.downloaded_bytes / 1_000_000).toFixed(1) }} / 612.1 MB stored
        <span v-if="status.elapsed_ms > 0"> · {{ (status.elapsed_ms / 1000).toFixed(1) }} seconds</span>
      </p>
      <progress v-if="status.phase === 'downloading'" class="w-full" :value="percentage" max="100" aria-label="Test key download" />
    </div>
    <AppAlert v-if="error || status.error" variant="error">{{ error || status.error }}</AppAlert>
    <AppAlert v-if="status.result" variant="success">
      Synthetic proof verified in {{ (status.result.elapsed_ms / 1000).toFixed(2) }} seconds.
      Prover peak memory: {{ (status.result.peak_rss_bytes / 1048576).toFixed(0) }} MiB.
    </AppAlert>
    <div class="flex flex-wrap gap-2">
      <AppButton v-if="status.key_status !== 'stored'" :disabled="busy || pending || privateReceipts.pending.value" @click="lab.act('download')">
        {{ status.key_status === 'partial' ? 'Resume key download' : 'Download test key · 612 MB' }}
      </AppButton>
      <AppButton v-else :disabled="busy || pending || privateReceipts.pending.value" @click="lab.act('prove')">Run synthetic test</AppButton>
      <AppButton v-if="busy || pending || privateReceipts.pending.value" variant="outline" :disabled="status.phase === 'cancelling'" @click="privateReceipts.cancel(); lab.act('cancel')">Cancel</AppButton>
      <AppButton v-if="status.key_status !== 'missing'" variant="outline" :disabled="busy || pending || privateReceipts.pending.value" @click="lab.act('remove_key')">Remove test key</AppButton>
    </div>
    <div class="space-y-3 border-t border-border pt-4">
      <h4 class="font-medium text-foreground">Private synthetic receipts</h4>
      <p class="text-sm text-muted-foreground">
        Bind a fresh synthetic proof to this unlocked profile and save a private diagnostic receipt.
        This does not verify a real person or change account permissions. Receipts stay on this device.
      </p>
      <p v-if="!isUnlocked" class="text-sm text-muted-foreground">Create or unlock a profile to test account binding.</p>
      <AppButton v-else :disabled="busy || pending || privateReceipts.pending.value || status.key_status !== 'stored'" @click="privateReceipts.start()">
        {{ privateReceipts.pending.value ? 'Verifying private receipt…' : 'Create private synthetic receipt' }}
      </AppButton>
      <AppAlert v-if="privateReceipts.error.value" variant="error">{{ privateReceipts.error.value }}</AppAlert>
      <ul v-if="isUnlocked" class="space-y-2 text-sm text-muted-foreground">
        <li v-for="receipt in privateReceipts.receipts.value" :key="receipt.id">
          Synthetic receipt · {{ new Date(receipt.created_at * 1000).toLocaleString() }}
          · expires {{ new Date(receipt.expires_at * 1000).toLocaleString() }}
        </li>
      </ul>
    </div>
  </section>
</template>
