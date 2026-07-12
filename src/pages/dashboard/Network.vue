<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue'
import { useI18n } from 'vue-i18n'
import { useP2P } from '@/composables/useP2P'
import type { PeerInfo } from '@/types'

const { t } = useI18n()

const { status: p2pStatus, lastError: p2pError, refreshStatus, peers: fetchPeers, startPolling, stopPolling } = useP2P()

const peerList = ref<PeerInfo[]>([])
const loading = ref(true)
const mountError = ref<string | null>(null)

const isRunning = computed(() => p2pStatus.value?.is_running ?? false)
const peerCount = computed(() => p2pStatus.value?.connected_peers ?? 0)
const topicCount = computed(() => p2pStatus.value?.subscribed_topics?.length ?? 0)

onMounted(async () => {
  try {
    await refreshStatus()
    startPolling(5000)
    if (isRunning.value) {
      peerList.value = await fetchPeers().catch(() => [])
    }
  } catch (e: unknown) {
    mountError.value = e instanceof Error ? e.message : String(e)
  } finally {
    loading.value = false
  }
})

onUnmounted(() => {
  stopPolling()
})

async function refreshPeers() {
  peerList.value = await fetchPeers().catch(() => [])
}
</script>

<template>
  <div class="space-y-4">
    <!-- Mount error -->
    <div v-if="mountError" style="background: #dc2626; color: white; padding: 16px; border-radius: 8px;">
      {{ t('network.mountError', { error: mountError }) }}
    </div>

    <!-- Header -->
    <div class="flex items-center gap-3">
      <div class="flex h-10 w-10 items-center justify-center rounded-lg bg-primary/10">
        <svg class="h-5 w-5 text-primary" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1.5" stroke="currentColor">
          <path stroke-linecap="round" stroke-linejoin="round" d="M12 21a9.004 9.004 0 0 0 8.716-6.747M12 21a9.004 9.004 0 0 1-8.716-6.747M12 21c2.485 0 4.5-4.03 4.5-9S14.485 3 12 3m0 18c-2.485 0-4.5-4.03-4.5-9S9.515 3 12 3m0 0a8.997 8.997 0 0 1 7.843 4.582M12 3a8.997 8.997 0 0 0-7.843 4.582m15.686 0A11.953 11.953 0 0 1 12 10.5c-2.998 0-5.74-1.1-7.843-2.918m15.686 0A8.959 8.959 0 0 1 21 12c0 .778-.099 1.533-.284 2.253m0 0A17.919 17.919 0 0 1 12 16.5a17.92 17.92 0 0 1-8.716-2.247m0 0A8.966 8.966 0 0 1 3 12c0-1.264.26-2.467.732-3.558" />
        </svg>
      </div>
      <div>
        <h1 class="text-2xl font-bold text-foreground">{{ t('network.title') }}</h1>
        <p class="text-sm text-muted-foreground">{{ t('network.subtitle') }}</p>
      </div>
    </div>

    <!-- Loading skeleton -->
    <div v-if="loading" class="grid grid-cols-1 gap-4 sm:grid-cols-3">
      <div v-for="i in 3" :key="i" class="animate-pulse rounded-xl border border-border bg-card p-5">
        <div class="h-3 w-20 rounded bg-muted-foreground/15 mb-3" />
        <div class="h-7 w-16 rounded bg-muted-foreground/15" />
      </div>
    </div>

    <!-- Stats cards -->
    <div v-else class="grid grid-cols-1 gap-4 sm:grid-cols-3">
      <!-- Status -->
      <div class="rounded-xl border border-border bg-card p-5">
        <div class="text-xs font-medium uppercase tracking-wider text-muted-foreground">{{ t('network.status') }}</div>
        <div class="mt-2 flex items-center gap-2">
          <span
            class="inline-block h-2.5 w-2.5 rounded-full"
            :class="isRunning ? 'bg-success' : 'bg-warning animate-pulse'"
          />
          <span class="text-xl font-bold text-foreground">
            {{ isRunning ? t('network.online') : p2pStatus != null ? t('network.offline') : t('network.starting') }}
          </span>
        </div>
      </div>

      <!-- Connections -->
      <div class="rounded-xl border border-border bg-card p-5">
        <div class="text-xs font-medium uppercase tracking-wider text-muted-foreground">{{ t('network.connections') }}</div>
        <div class="mt-2">
          <span class="text-xl font-bold text-foreground">{{ peerCount }}</span>
        </div>
      </div>

      <!-- Channels -->
      <div class="rounded-xl border border-border bg-card p-5">
        <div class="text-xs font-medium uppercase tracking-wider text-muted-foreground">{{ t('network.advanced.topics') }}</div>
        <div class="mt-2">
          <span class="text-xl font-bold text-foreground">{{ topicCount || '\u2014' }}</span>
        </div>
      </div>
    </div>

    <!-- Advanced connection details -->
    <details class="rounded-xl border border-border bg-card p-5">
      <summary class="cursor-pointer text-base font-semibold text-foreground">{{ t('common.advanced.toggle') }}</summary>
      <p class="mt-1 mb-3 text-xs text-muted-foreground">{{ t('common.advanced.hint') }}</p>

      <div class="divide-y divide-border/50">
        <div class="flex items-center justify-between py-2.5">
          <span class="text-sm text-muted-foreground">{{ t('network.advanced.deviceId') }}</span>
          <span class="text-xs font-mono text-foreground max-w-[60%] truncate">
            {{ p2pStatus?.peer_id ?? '\u2014' }}
          </span>
        </div>
        <div class="flex items-center justify-between py-2.5">
          <span class="text-sm text-muted-foreground">{{ t('network.connections') }}</span>
          <span class="text-sm font-medium text-foreground">{{ peerCount }}</span>
        </div>
        <div class="flex items-center justify-between py-2.5">
          <span class="text-sm text-muted-foreground">{{ t('network.advanced.topics') }}</span>
          <span class="text-xs font-mono text-foreground">
            {{ p2pStatus?.subscribed_topics?.length ? p2pStatus.subscribed_topics.join(', ') : '\u2014' }}
          </span>
        </div>
        <div class="flex items-start justify-between py-2.5">
          <span class="text-sm text-muted-foreground shrink-0">{{ t('network.advanced.addresses') }}</span>
          <div v-if="p2pStatus?.listening_addresses?.length" class="text-end ms-4">
            <div
              v-for="addr in p2pStatus.listening_addresses"
              :key="addr"
              class="text-xs font-mono text-foreground break-all leading-relaxed"
            >
              {{ addr }}
            </div>
          </div>
          <span v-else class="text-sm text-foreground">&mdash;</span>
        </div>
        <div v-if="p2pStatus?.nat_status" class="flex items-center justify-between py-2.5">
          <span class="text-sm text-muted-foreground">{{ t('network.advanced.natStatus') }}</span>
          <span class="text-sm font-medium text-foreground">{{ p2pStatus.nat_status }}</span>
        </div>
      </div>
    </details>

    <!-- Diagnostic (show when offline or error) -->
    <div v-if="p2pError || !isRunning" class="rounded-xl border border-amber-500/30 bg-amber-50 dark:bg-amber-500/5 p-4">
      <h3 class="text-sm font-semibold text-amber-700 dark:text-amber-400 mb-2">{{ t('network.diagnostic') }}</h3>
      <div v-if="p2pError" class="text-xs font-mono text-red-600 dark:text-red-400 mb-2">{{ p2pError }}</div>
      <pre class="text-xs font-mono text-muted-foreground whitespace-pre-wrap break-all">{{ JSON.stringify(p2pStatus, null, 2) ?? 'null' }}</pre>
    </div>

    <!-- Connected Peers list -->
    <div class="rounded-xl border border-border bg-card p-5">
      <div class="flex items-center justify-between mb-3">
        <h2 class="text-base font-semibold text-foreground">{{ t('network.connections') }}</h2>
        <button
          class="text-sm text-primary hover:text-primary-hover font-medium"
          @click="refreshPeers"
        >
          {{ t('network.refresh') }}
        </button>
      </div>

      <div v-if="peerList.length > 0" class="space-y-3">
        <div
          v-for="peer in peerList"
          :key="peer"
          class="rounded-lg border border-border p-3"
        >
          <div class="flex items-start gap-2">
            <span class="mt-1 inline-block h-2 w-2 shrink-0 rounded-full bg-emerald-500" />
            <div class="min-w-0 flex-1">
              <div class="text-sm font-mono text-foreground break-all">{{ peer }}</div>
            </div>
          </div>
        </div>
      </div>

      <div v-else class="text-center py-8">
        <p class="text-sm text-muted-foreground">{{ t('network.empty') }}</p>
      </div>
    </div>
  </div>
</template>
