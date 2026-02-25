<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useP2P } from '@/composables/useP2P'
import { AppButton } from '@/components/ui'
import type { PeerInfo } from '@/types'

const { status: p2pStatus, start, stop, peers: fetchPeers, refreshStatus } = useP2P()

const peerList = ref<PeerInfo[]>([])
const loading = ref(true)
const toggling = ref(false)

const isRunning = computed(() => p2pStatus.value?.running ?? false)
const peerCount = computed(() => p2pStatus.value?.connected_peers ?? 0)
const topicCount = computed(() => p2pStatus.value?.gossipsub_topics?.length ?? 0)

onMounted(async () => {
  await refreshStatus()
  if (p2pStatus.value?.running) {
    try {
      peerList.value = await fetchPeers()
    } catch { /* no peers */ }
  }
  loading.value = false
})

async function toggle() {
  toggling.value = true
  try {
    if (p2pStatus.value?.running) {
      await stop()
      peerList.value = []
    } else {
      await start()
      peerList.value = await fetchPeers().catch(() => [])
    }
  } catch (e) {
    console.error('Failed to toggle P2P:', e)
  } finally {
    toggling.value = false
  }
}

async function refreshPeers() {
  peerList.value = await fetchPeers().catch(() => [])
}
</script>

<template>
  <div>
    <!-- Header -->
    <div class="py-8 px-4 sm:px-6 lg:px-8">
      <div class="flex items-center justify-between">
        <div class="flex items-center gap-3">
          <div class="flex h-10 w-10 items-center justify-center rounded-lg bg-[rgb(var(--color-primary)/0.1)]">
            <svg class="h-5 w-5 text-[rgb(var(--color-primary))]" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1.5" stroke="currentColor">
              <path stroke-linecap="round" stroke-linejoin="round" d="M12 21a9.004 9.004 0 0 0 8.716-6.747M12 21a9.004 9.004 0 0 1-8.716-6.747M12 21c2.485 0 4.5-4.03 4.5-9S14.485 3 12 3m0 18c-2.485 0-4.5-4.03-4.5-9S9.515 3 12 3m0 0a8.997 8.997 0 0 1 7.843 4.582M12 3a8.997 8.997 0 0 0-7.843 4.582m15.686 0A11.953 11.953 0 0 1 12 10.5c-2.998 0-5.74-1.1-7.843-2.918m15.686 0A8.959 8.959 0 0 1 21 12c0 .778-.099 1.533-.284 2.253m0 0A17.919 17.919 0 0 1 12 16.5a17.92 17.92 0 0 1-8.716-2.247m0 0A8.966 8.966 0 0 1 3 12c0-1.264.26-2.467.732-3.558" />
            </svg>
          </div>
          <div>
            <h1 class="text-3xl font-bold text-[rgb(var(--color-foreground))]">P2P Network</h1>
            <p class="text-sm text-[rgb(var(--color-muted-foreground))] mt-1">
              Manage your node's peer-to-peer network connection.
            </p>
          </div>
        </div>
        <AppButton
          :variant="isRunning ? 'outline' : 'primary'"
          :loading="toggling"
          @click="toggle"
        >
          <template v-if="!toggling">
            <svg v-if="isRunning" class="h-4 w-4 mr-2" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1.5" stroke="currentColor">
              <path stroke-linecap="round" stroke-linejoin="round" d="M5.25 7.5A2.25 2.25 0 0 1 7.5 5.25h9a2.25 2.25 0 0 1 2.25 2.25v9a2.25 2.25 0 0 1-2.25 2.25h-9a2.25 2.25 0 0 1-2.25-2.25v-9Z" />
            </svg>
            <svg v-else class="h-4 w-4 mr-2" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1.5" stroke="currentColor">
              <path stroke-linecap="round" stroke-linejoin="round" d="M5.25 5.653c0-.856.917-1.398 1.667-.986l11.54 6.347a1.125 1.125 0 0 1 0 1.972l-11.54 6.347a1.125 1.125 0 0 1-1.667-.986V5.653Z" />
            </svg>
          </template>
          {{ isRunning ? 'Stop Node' : 'Start Node' }}
        </AppButton>
      </div>
    </div>

    <!-- Skeleton loader -->
    <div v-if="loading" class="px-4 sm:px-6 lg:px-8 space-y-6">
      <!-- Stats skeleton -->
      <div class="grid grid-cols-1 sm:grid-cols-3 gap-4">
        <div v-for="i in 3" :key="i" class="rounded-xl border border-[rgb(var(--color-border))] bg-[rgb(var(--color-card))] p-6">
          <div class="animate-pulse space-y-3">
            <div class="h-3 w-20 rounded bg-[rgb(var(--color-muted-foreground)/0.15)]" />
            <div class="h-7 w-16 rounded bg-[rgb(var(--color-muted-foreground)/0.15)]" />
          </div>
        </div>
      </div>
      <!-- Card skeletons -->
      <div v-for="i in 2" :key="i" class="rounded-xl border border-[rgb(var(--color-border))] bg-[rgb(var(--color-card))] p-6">
        <div class="animate-pulse space-y-4">
          <div class="h-4 w-32 rounded bg-[rgb(var(--color-muted-foreground)/0.15)]" />
          <div class="space-y-3">
            <div class="h-3 w-full rounded bg-[rgb(var(--color-muted-foreground)/0.1)]" />
            <div class="h-3 w-3/4 rounded bg-[rgb(var(--color-muted-foreground)/0.1)]" />
            <div class="h-3 w-1/2 rounded bg-[rgb(var(--color-muted-foreground)/0.1)]" />
          </div>
        </div>
      </div>
    </div>

    <!-- Loaded content -->
    <div v-else class="px-4 sm:px-6 lg:px-8 space-y-6">
      <!-- Stats grid -->
      <div class="grid grid-cols-1 sm:grid-cols-3 gap-4">
        <!-- Status -->
        <div class="rounded-xl border border-[rgb(var(--color-border))] bg-[rgb(var(--color-card))] p-6">
          <div class="text-xs font-medium uppercase tracking-wider text-[rgb(var(--color-muted-foreground))]">Status</div>
          <div class="mt-2 flex items-center gap-2">
            <span
              class="inline-block h-2.5 w-2.5 rounded-full"
              :class="isRunning ? 'bg-emerald-500 shadow-[0_0_6px_rgb(16,185,129,0.4)]' : 'bg-[rgb(var(--color-muted-foreground)/0.4)]'"
            />
            <span class="text-2xl font-bold text-[rgb(var(--color-foreground))]">
              {{ isRunning ? 'Online' : 'Offline' }}
            </span>
          </div>
        </div>

        <!-- Connected Peers -->
        <div class="rounded-xl border border-[rgb(var(--color-border))] bg-[rgb(var(--color-card))] p-6">
          <div class="text-xs font-medium uppercase tracking-wider text-[rgb(var(--color-muted-foreground))]">Connected Peers</div>
          <div class="mt-2">
            <span class="text-2xl font-bold text-[rgb(var(--color-foreground))]">{{ peerCount }}</span>
          </div>
        </div>

        <!-- Topics -->
        <div class="rounded-xl border border-[rgb(var(--color-border))] bg-[rgb(var(--color-card))] p-6">
          <div class="text-xs font-medium uppercase tracking-wider text-[rgb(var(--color-muted-foreground))]">Topics</div>
          <div class="mt-2">
            <span class="text-2xl font-bold text-[rgb(var(--color-foreground))]">{{ topicCount || '\u2014' }}</span>
          </div>
        </div>
      </div>

      <!-- Node Status card -->
      <div class="rounded-xl border border-[rgb(var(--color-border))] bg-[rgb(var(--color-card))] p-6">
        <h2 class="text-base font-semibold text-[rgb(var(--color-foreground))] mb-4">Node Status</h2>

        <div>
          <!-- Peer ID -->
          <div class="flex items-center justify-between py-2 border-b border-[rgb(var(--color-border)/0.5)] last:border-0">
            <span class="text-sm text-[rgb(var(--color-muted-foreground))]">Peer ID</span>
            <span class="text-sm font-medium text-[rgb(var(--color-foreground))] font-mono text-xs max-w-[60%] truncate" :title="p2pStatus?.peer_id ?? undefined">
              {{ p2pStatus?.peer_id ?? '\u2014' }}
            </span>
          </div>

          <!-- Connected Peers -->
          <div class="flex items-center justify-between py-2 border-b border-[rgb(var(--color-border)/0.5)] last:border-0">
            <span class="text-sm text-[rgb(var(--color-muted-foreground))]">Connected Peers</span>
            <span class="text-sm font-medium text-[rgb(var(--color-foreground))]">{{ peerCount }}</span>
          </div>

          <!-- Topics -->
          <div class="flex items-center justify-between py-2 border-b border-[rgb(var(--color-border)/0.5)] last:border-0">
            <span class="text-sm text-[rgb(var(--color-muted-foreground))]">Topics</span>
            <span class="text-sm font-medium text-[rgb(var(--color-foreground))] font-mono text-xs">
              {{ p2pStatus?.gossipsub_topics?.length ? p2pStatus.gossipsub_topics.join(', ') : '\u2014' }}
            </span>
          </div>

          <!-- Listening Addresses -->
          <div class="flex items-start justify-between py-2 last:border-0">
            <span class="text-sm text-[rgb(var(--color-muted-foreground))] shrink-0 pt-0.5">Listening Addresses</span>
            <div v-if="p2pStatus?.listening_addresses?.length" class="text-right ml-4">
              <div
                v-for="addr in p2pStatus.listening_addresses"
                :key="addr"
                class="font-mono text-xs text-[rgb(var(--color-foreground))] break-all leading-relaxed"
              >
                {{ addr }}
              </div>
            </div>
            <span v-else class="text-sm font-medium text-[rgb(var(--color-foreground))]">&mdash;</span>
          </div>
        </div>
      </div>

      <!-- Peers section -->
      <div class="rounded-xl border border-[rgb(var(--color-border))] bg-[rgb(var(--color-card))] p-6">
        <div class="flex items-center justify-between mb-4">
          <h2 class="text-base font-semibold text-[rgb(var(--color-foreground))]">Connected Peers</h2>
          <AppButton variant="ghost" size="sm" @click="refreshPeers">
            <svg class="h-4 w-4 mr-1.5" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1.5" stroke="currentColor">
              <path stroke-linecap="round" stroke-linejoin="round" d="M16.023 9.348h4.992v-.001M2.985 19.644v-4.992m0 0h4.992m-4.993 0 3.181 3.183a8.25 8.25 0 0 0 13.803-3.7M4.031 9.865a8.25 8.25 0 0 1 13.803-3.7l3.181 3.182" />
            </svg>
            Refresh
          </AppButton>
        </div>

        <!-- Peer cards -->
        <div v-if="peerList.length > 0" class="space-y-3">
          <div
            v-for="peer in peerList"
            :key="peer.peer_id"
            class="rounded-lg border border-[rgb(var(--color-border))] p-4 transition-colors hover:border-[rgb(var(--color-primary)/0.3)]"
          >
            <div class="flex items-start gap-3">
              <span class="mt-1.5 inline-block h-2 w-2 shrink-0 rounded-full bg-emerald-500 shadow-[0_0_6px_rgb(16,185,129,0.4)]" />
              <div class="min-w-0 flex-1">
                <div class="font-mono text-sm text-[rgb(var(--color-foreground))] break-all leading-snug">
                  {{ peer.peer_id }}
                </div>
                <div v-if="peer.addresses.length" class="mt-1.5 space-y-0.5">
                  <div
                    v-for="addr in peer.addresses"
                    :key="addr"
                    class="font-mono text-xs text-[rgb(var(--color-muted-foreground))] break-all"
                  >
                    {{ addr }}
                  </div>
                </div>
              </div>
            </div>
          </div>
        </div>

        <!-- Empty state -->
        <div v-else class="flex flex-col items-center justify-center py-12">
          <div class="flex h-16 w-16 items-center justify-center rounded-full bg-[rgb(var(--color-muted)/0.3)] mb-4">
            <svg class="h-8 w-8 text-[rgb(var(--color-muted-foreground)/0.5)]" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1.5" stroke="currentColor">
              <path stroke-linecap="round" stroke-linejoin="round" d="M12 21a9.004 9.004 0 0 0 8.716-6.747M12 21a9.004 9.004 0 0 1-8.716-6.747M12 21c2.485 0 4.5-4.03 4.5-9S14.485 3 12 3m0 18c-2.485 0-4.5-4.03-4.5-9S9.515 3 12 3m0 0a8.997 8.997 0 0 1 7.843 4.582M12 3a8.997 8.997 0 0 0-7.843 4.582m15.686 0A11.953 11.953 0 0 1 12 10.5c-2.998 0-5.74-1.1-7.843-2.918m15.686 0A8.959 8.959 0 0 1 21 12c0 .778-.099 1.533-.284 2.253m0 0A17.919 17.919 0 0 1 12 16.5a17.92 17.92 0 0 1-8.716-2.247m0 0A8.966 8.966 0 0 1 3 12c0-1.264.26-2.467.732-3.558" />
            </svg>
          </div>
          <p class="text-sm font-medium text-[rgb(var(--color-foreground))]">No peers connected</p>
          <p class="text-sm text-[rgb(var(--color-muted-foreground))] mt-1 text-center max-w-xs">
            {{ isRunning ? 'Waiting for peer discovery via mDNS and DHT...' : 'Start the P2P network to discover peers.' }}
          </p>
        </div>
      </div>
    </div>
  </div>
</template>
