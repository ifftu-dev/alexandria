<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useLocalApi } from '@/composables/useLocalApi'
import type { SyncStatus, DeviceInfo, SyncHistoryEntry, SyncResult } from '@/types'

const { invoke } = useLocalApi()

const status = ref<SyncStatus | null>(null)
const device = ref<DeviceInfo | null>(null)
const history = ref<SyncHistoryEntry[]>([])
const loading = ref(true)
const syncing = ref(false)
const syncResult = ref<SyncResult | null>(null)

const deviceCount = computed(() => status.value?.device_count ?? 0)
const queueLength = computed(() => status.value?.queue_length ?? 0)
const isAutoSync = computed(() => status.value?.auto_sync ?? false)

onMounted(async () => {
  try {
    const [s, d, h] = await Promise.all([
      invoke<SyncStatus>('sync_status'),
      invoke<DeviceInfo>('sync_get_device_info'),
      invoke<SyncHistoryEntry[]>('sync_history'),
    ])
    status.value = s
    device.value = d
    history.value = h
  } catch (e) {
    console.error('Failed to load sync data:', e)
  } finally {
    loading.value = false
  }
})

async function syncNow() {
  syncing.value = true
  syncResult.value = null
  try {
    syncResult.value = await invoke<SyncResult>('sync_now')
    status.value = await invoke<SyncStatus>('sync_status')
    history.value = await invoke<SyncHistoryEntry[]>('sync_history')
  } catch (e) {
    console.error('Sync failed:', e)
  } finally {
    syncing.value = false
  }
}

async function toggleAutoSync() {
  if (!status.value) return
  try {
    await invoke('sync_set_auto', { enabled: !status.value.auto_sync })
    status.value = await invoke<SyncStatus>('sync_status')
  } catch (e) {
    console.error('Failed to toggle auto-sync:', e)
  }
}
</script>

<template>
  <div>
    <!-- Skeleton loader -->
    <div v-if="loading" class="animate-pulse">
      <!-- Header skeleton -->
      <div>
        <div class="h-8 w-56 rounded bg-muted" />
        <div class="mt-2 h-4 w-80 rounded bg-muted/60" />
      </div>
      <!-- Stats skeleton -->
      <div class="px-4 sm:px-6 lg:px-8 grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
        <div v-for="i in 4" :key="i" class="rounded-lg border border-border p-5">
          <div class="h-3 w-16 rounded bg-muted/50" />
          <div class="mt-3 h-8 w-12 rounded bg-muted" />
        </div>
      </div>
      <!-- Card skeletons -->
      <div class="mt-8 px-4 sm:px-6 lg:px-8 space-y-6">
        <div class="rounded-lg border border-border p-5 space-y-3">
          <div class="h-5 w-28 rounded bg-muted" />
          <div v-for="j in 3" :key="j" class="h-10 rounded bg-muted/30" />
        </div>
        <div class="rounded-lg border border-border p-5 space-y-3">
          <div class="h-5 w-32 rounded bg-muted" />
          <div class="grid gap-3 sm:grid-cols-2">
            <div v-for="k in 2" :key="k" class="h-20 rounded-lg bg-muted/20" />
          </div>
        </div>
        <div class="rounded-lg border border-border p-5 space-y-3">
          <div class="h-5 w-28 rounded bg-muted" />
          <div v-for="l in 4" :key="l" class="h-12 rounded-lg bg-muted/15" />
        </div>
      </div>
    </div>

    <!-- Loaded content -->
    <template v-else>
      <!-- Header -->
      <div>
        <div class="flex items-start justify-between gap-4">
          <div>
            <h1 class="text-3xl font-bold tracking-tight text-foreground">
              Cross-Device Sync
            </h1>
            <p class="mt-1.5 text-sm text-muted-foreground">
              Keep your data synchronized across every device paired with the same recovery phrase.
            </p>
          </div>
          <button
            :disabled="syncing"
            class="relative inline-flex items-center gap-2 rounded-lg bg-primary px-5 py-2.5 text-sm font-medium text-white shadow-sm transition-all hover:bg-primary/90 disabled:opacity-60 disabled:cursor-not-allowed"
            @click="syncNow"
          >
            <svg
              v-if="syncing"
              class="h-4 w-4 animate-spin"
              fill="none"
              viewBox="0 0 24 24"
            >
              <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4" />
              <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z" />
            </svg>
            <svg
              v-else
              class="h-4 w-4"
              fill="none"
              viewBox="0 0 24 24"
              stroke="currentColor"
              stroke-width="2"
            >
              <path stroke-linecap="round" stroke-linejoin="round" d="M16.023 9.348h4.992v-.001M2.985 19.644v-4.992m0 0h4.992m-4.993 0l3.181 3.183a8.25 8.25 0 0013.803-3.7M4.031 9.865a8.25 8.25 0 0113.803-3.7l3.181 3.182M21.015 4.356v4.992" />
            </svg>
            {{ syncing ? 'Syncing...' : 'Sync Now' }}
          </button>
        </div>
      </div>

      <!-- Stats grid -->
      <div class="px-4 sm:px-6 lg:px-8 grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
        <!-- Devices -->
        <div class="rounded-lg border border-border bg-card p-5">
          <p class="text-xs font-medium uppercase tracking-wider text-muted-foreground">Devices</p>
          <p class="mt-2 text-3xl font-bold tabular-nums text-foreground">{{ deviceCount }}</p>
        </div>

        <!-- Queue -->
        <div class="rounded-lg border border-border bg-card p-5">
          <p class="text-xs font-medium uppercase tracking-wider text-muted-foreground">Queue</p>
          <p class="mt-2 text-3xl font-bold tabular-nums text-foreground">
            {{ queueLength }}
            <span class="text-sm font-normal text-muted-foreground">pending</span>
          </p>
        </div>

        <!-- Auto-sync toggle -->
        <div class="rounded-lg border border-border bg-card p-5">
          <p class="text-xs font-medium uppercase tracking-wider text-muted-foreground">Auto-Sync</p>
          <div class="mt-2 flex items-center gap-3">
            <button
              role="switch"
              :aria-checked="isAutoSync"
              class="relative inline-flex h-6 w-11 shrink-0 cursor-pointer rounded-full border-2 border-transparent transition-colors duration-200 ease-in-out focus:outline-none focus-visible:ring-2 focus-visible:ring-primary focus-visible:ring-offset-2"
              :class="isAutoSync ? 'bg-primary' : 'bg-muted/40'"
              @click="toggleAutoSync"
            >
              <span
                class="pointer-events-none inline-block h-5 w-5 rounded-full bg-white shadow-lg ring-0 transition-transform duration-200 ease-in-out"
                :class="isAutoSync ? 'translate-x-5' : 'translate-x-0'"
              />
            </button>
            <span class="text-lg font-semibold text-foreground">
              {{ isAutoSync ? 'On' : 'Off' }}
            </span>
          </div>
        </div>

        <!-- Last Sync -->
        <div class="rounded-lg border border-border bg-card p-5">
          <p class="text-xs font-medium uppercase tracking-wider text-muted-foreground">Last Sync</p>
          <p class="mt-2 text-lg font-semibold text-foreground">
            {{ status?.last_sync ?? 'Never' }}
          </p>
        </div>
      </div>

      <!-- Sync result banner -->
      <div
        v-if="syncResult"
        class="mx-4 sm:mx-6 lg:mx-8 mt-6 rounded-lg border border-emerald-500/20 bg-emerald-500/10 p-4"
      >
        <div class="flex items-start gap-3">
          <svg
            class="mt-0.5 h-5 w-5 shrink-0 text-emerald-500"
            fill="none"
            viewBox="0 0 24 24"
            stroke="currentColor"
            stroke-width="2"
          >
            <path stroke-linecap="round" stroke-linejoin="round" d="M9 12.75L11.25 15 15 9.75M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
          </svg>
          <div>
            <p class="text-sm font-medium text-emerald-700 dark:text-emerald-300">
              Sync completed successfully
            </p>
            <p class="mt-0.5 text-xs text-emerald-600 dark:text-emerald-400">
              Merged {{ syncResult.rows_merged }} rows in {{ syncResult.duration_ms }}ms
              &mdash; sent {{ syncResult.rows_sent }}, received {{ syncResult.rows_received }}
            </p>
          </div>
        </div>
      </div>

      <!-- Device Info card -->
      <div class="mt-8 px-4 sm:px-6 lg:px-8">
        <div class="rounded-lg border border-border bg-card p-5">
          <h2 class="text-sm font-semibold uppercase tracking-wider text-muted-foreground mb-4">
            This Device
          </h2>

          <div class="divide-y divide-border/50">
            <!-- Device ID -->
            <div class="flex items-center justify-between py-2.5">
              <span class="text-sm text-muted-foreground">Device ID</span>
              <span class="font-mono text-sm text-foreground select-all">
                {{ device?.id ?? '...' }}
              </span>
            </div>

            <!-- Name -->
            <div v-if="device?.device_name" class="flex items-center justify-between py-2.5">
              <span class="text-sm text-muted-foreground">Name</span>
              <span class="text-sm font-medium text-foreground">
                {{ device.device_name }}
              </span>
            </div>

            <!-- Platform -->
            <div class="flex items-center justify-between py-2.5">
              <span class="text-sm text-muted-foreground">Platform</span>
              <span class="text-sm text-foreground">
                {{ device?.platform ?? 'Unknown' }}
              </span>
            </div>
          </div>
        </div>
      </div>

      <!-- Paired Devices -->
      <div class="mt-6 px-4 sm:px-6 lg:px-8">
        <h2 class="text-sm font-semibold uppercase tracking-wider text-muted-foreground mb-4">
          Paired Devices
        </h2>

        <!-- Rich empty state -->
        <div
          v-if="!status?.devices?.length"
          class="rounded-xl border border-border bg-card p-12 text-center"
        >
          <div class="mx-auto mb-4 flex h-14 w-14 items-center justify-center rounded-full bg-muted/15">
            <svg
              class="h-7 w-7 text-muted-foreground"
              fill="none"
              viewBox="0 0 24 24"
              stroke="currentColor"
              stroke-width="1.5"
            >
              <path stroke-linecap="round" stroke-linejoin="round" d="M10.5 1.5H8.25A2.25 2.25 0 006 3.75v16.5a2.25 2.25 0 002.25 2.25h7.5A2.25 2.25 0 0018 20.25V3.75a2.25 2.25 0 00-2.25-2.25H13.5m-3 0V3h3V1.5m-3 0h3m-3 18.75h3" />
            </svg>
          </div>
          <p class="text-sm font-medium text-foreground">No other devices paired</p>
          <p class="mt-1 text-xs text-muted-foreground max-w-sm mx-auto">
            Import the same recovery phrase on another device to pair it with this node and start syncing automatically.
          </p>
        </div>

        <!-- Device cards -->
        <div v-else class="grid gap-3 sm:grid-cols-2">
          <div
            v-for="d in status.devices"
            :key="d.device_id"
            class="rounded-lg border border-border bg-card p-4 transition-all hover:border-border/80 hover:shadow-sm"
          >
            <div class="flex items-start justify-between gap-3">
              <div class="min-w-0">
                <p class="truncate text-sm font-medium text-foreground">
                  {{ d.device_name ?? d.device_id }}
                </p>
                <p class="mt-0.5 text-xs text-muted-foreground">
                  {{ d.tables_synced }} tables synced
                  <template v-if="d.last_synced">
                    &middot; last {{ d.last_synced }}
                  </template>
                </p>
              </div>
              <span class="flex items-center gap-1.5 shrink-0">
                <span
                  class="h-2 w-2 rounded-full"
                  :class="d.is_online ? 'bg-emerald-500' : 'bg-muted-foreground/30'"
                />
                <span class="text-xs text-muted-foreground">
                  {{ d.is_online ? 'Online' : 'Offline' }}
                </span>
              </span>
            </div>
          </div>
        </div>
      </div>

      <!-- Sync History -->
      <div class="mt-8 mb-8 px-4 sm:px-6 lg:px-8">
        <h2 class="text-sm font-semibold uppercase tracking-wider text-muted-foreground mb-4">
          Sync History
        </h2>

        <!-- Empty history -->
        <div
          v-if="history.length === 0"
          class="rounded-xl border border-border bg-card p-12 text-center"
        >
          <div class="mx-auto mb-4 flex h-14 w-14 items-center justify-center rounded-full bg-muted/15">
            <svg
              class="h-7 w-7 text-muted-foreground"
              fill="none"
              viewBox="0 0 24 24"
              stroke="currentColor"
              stroke-width="1.5"
            >
              <path stroke-linecap="round" stroke-linejoin="round" d="M12 6v6h4.5m4.5 0a9 9 0 11-18 0 9 9 0 0118 0z" />
            </svg>
          </div>
          <p class="text-sm font-medium text-foreground">No sync history</p>
          <p class="mt-1 text-xs text-muted-foreground">
            Sync history will appear here after your first sync.
          </p>
        </div>

        <!-- Timeline entries -->
        <div v-else class="space-y-2">
          <div
            v-for="(entry, i) in history.slice(0, 20)"
            :key="i"
            class="rounded-lg bg-muted/15 px-4 py-3"
          >
            <div class="flex items-center justify-between gap-4">
              <div class="flex items-center gap-3 min-w-0">
                <span class="text-sm font-semibold text-foreground truncate">
                  {{ entry.device_name ?? entry.device_id }}
                </span>
                <span
                  class="inline-flex items-center rounded-full px-2 py-0.5 text-[0.65rem] font-medium uppercase tracking-wide"
                  :class="
                    entry.direction === 'push'
                      ? 'bg-blue-500/10 text-blue-600 dark:text-blue-400'
                      : entry.direction === 'pull'
                        ? 'bg-amber-500/10 text-amber-600 dark:text-amber-400'
                        : 'bg-muted/30 text-muted-foreground'
                  "
                >
                  {{ entry.direction }}
                </span>
              </div>
              <div class="flex items-center gap-3 shrink-0 text-xs text-muted-foreground">
                <span class="tabular-nums">{{ entry.rows_sent }}&uarr;</span>
                <span class="tabular-nums">{{ entry.rows_received }}&darr;</span>
                <span class="hidden sm:inline">&middot;</span>
                <span class="hidden sm:inline">{{ entry.synced_at }}</span>
              </div>
            </div>
            <p class="mt-0.5 text-xs text-muted-foreground sm:hidden">
              {{ entry.synced_at }}
            </p>
          </div>
        </div>
      </div>
    </template>
  </div>
</template>
