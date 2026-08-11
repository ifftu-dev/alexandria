<script setup lang="ts">
/**
 * "Install CLI" dialog (Developer menu).
 *
 * Stays hidden until the native Develop menu emits `develop://install-cli`,
 * so it is safe to always mount — same arrangement as SentinelDebugPip.
 *
 * Release builds ship the `alexandria` CLI beside the app binary and
 * installing symlinks it onto PATH, which is what lets the auto-updater carry
 * the CLI along: an update replaces the bundle the link points into. Dev
 * builds have no bundled CLI, so the backend compiles one from the working
 * tree instead — that takes minutes, hence the streamed log below rather than
 * an indeterminate spinner.
 *
 * Dev-only surface: listed in scripts/i18n/check-no-raw-text.mjs FILE_SKIP,
 * matching the precedent set by SentinelDebugPip.
 */
import { ref, onMounted, onBeforeUnmount, nextTick } from 'vue'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { useLocalApi } from '@/composables/useLocalApi'

interface CliInstallStatus {
  installed: boolean
  installPath: string
  linksTo: string | null
  bundledAvailable: boolean
  bundledPath: string | null
  sourceAvailable: boolean
  supported: boolean
  tracksUpdates: boolean
}

interface CliInstallResult {
  path: string
  source: string
  tracksUpdates: boolean
  message: string
}

const { invoke } = useLocalApi()

const open = ref(false)
const busy = ref(false)
const status = ref<CliInstallStatus | null>(null)
const log = ref<string[]>([])
const result = ref<CliInstallResult | null>(null)
const error = ref<string | null>(null)
const logEl = ref<HTMLElement | null>(null)

/** Keep the tail visible while a build streams. */
async function scrollLog() {
  await nextTick()
  if (logEl.value) logEl.value.scrollTop = logEl.value.scrollHeight
}

async function refreshStatus() {
  try {
    status.value = await invoke<CliInstallStatus>('cli_install_status')
  } catch (e) {
    error.value = String(e)
  }
}

async function show() {
  open.value = true
  error.value = null
  result.value = null
  log.value = []
  await refreshStatus()
}

async function install() {
  busy.value = true
  error.value = null
  result.value = null
  log.value = []
  try {
    result.value = await invoke<CliInstallResult>('cli_install')
    await refreshStatus()
  } catch (e) {
    error.value = String(e)
  } finally {
    busy.value = false
    await scrollLog()
  }
}

async function uninstall() {
  busy.value = true
  error.value = null
  result.value = null
  try {
    log.value = [await invoke<string>('cli_uninstall')]
    await refreshStatus()
  } catch (e) {
    error.value = String(e)
  } finally {
    busy.value = false
  }
}

let unlistenMenu: UnlistenFn | null = null
let unlistenProgress: UnlistenFn | null = null

onMounted(async () => {
  try {
    unlistenMenu = await listen('develop://install-cli', () => void show())
    unlistenProgress = await listen<string>('cli-install://progress', (event) => {
      log.value.push(event.payload)
      // A full cargo build emits thousands of lines; keeping every one costs
      // memory for output nobody scrolls back to.
      if (log.value.length > 500) log.value.splice(0, log.value.length - 500)
      void scrollLog()
    })
  } catch {
    /* not in a Tauri context */
  }
})

onBeforeUnmount(() => {
  unlistenMenu?.()
  unlistenProgress?.()
})
</script>

<template>
  <div
    v-if="open"
    class="fixed inset-0 z-[9998] flex items-center justify-center bg-black/50 p-4"
    @click.self="!busy && (open = false)"
  >
    <div
      class="w-full max-w-2xl rounded-lg border border-border bg-background shadow-xl"
      role="dialog"
      aria-modal="true"
    >
      <div class="flex items-center justify-between border-b border-border px-4 py-3">
        <h2 class="text-sm font-semibold">Install CLI</h2>
        <button
          class="text-muted-foreground hover:text-foreground disabled:opacity-50"
          :disabled="busy"
          aria-label="Close"
          @click="open = false"
        >
          ✕
        </button>
      </div>

      <div class="space-y-3 px-4 py-4 text-sm">
        <p v-if="status && !status.supported" class="text-destructive">
          Installing the CLI is only supported on macOS and Linux.
        </p>

        <template v-else-if="status">
          <div class="grid grid-cols-[9rem_1fr] gap-x-3 gap-y-1 font-mono text-xs">
            <span class="text-muted-foreground">Install path</span>
            <span class="break-all">{{ status.installPath }}</span>

            <span class="text-muted-foreground">Status</span>
            <span :class="status.installed ? 'text-green-600' : 'text-muted-foreground'">
              {{ status.installed ? 'installed' : 'not installed' }}
            </span>

            <template v-if="status.linksTo">
              <span class="text-muted-foreground">Links to</span>
              <span class="break-all">{{ status.linksTo }}</span>
            </template>

            <span class="text-muted-foreground">Source</span>
            <span>
              {{
                status.bundledAvailable
                  ? 'bundled with this app'
                  : status.sourceAvailable
                    ? 'build from source checkout'
                    : 'unavailable'
              }}
            </span>

            <span class="text-muted-foreground">App updates</span>
            <span :class="status.tracksUpdates ? 'text-green-600' : 'text-muted-foreground'">
              {{
                status.tracksUpdates
                  ? 'will update the CLI too'
                  : 'will not update the CLI'
              }}
            </span>
          </div>

          <p v-if="!status.bundledAvailable" class="text-xs text-muted-foreground">
            This build has no bundled CLI, so installing compiles it from the working
            tree. That takes several minutes and needs the Rust toolchain on PATH.
          </p>
        </template>

        <div
          v-if="log.length"
          ref="logEl"
          class="max-h-56 overflow-y-auto rounded border border-border bg-muted/40 p-2 font-mono text-[11px] leading-tight"
        >
          <div v-for="(line, i) in log" :key="i" class="whitespace-pre-wrap break-all">
            {{ line }}
          </div>
        </div>

        <p v-if="result" class="text-xs text-green-600">{{ result.message }}</p>
        <p v-if="error" class="whitespace-pre-wrap text-xs text-destructive">{{ error }}</p>
      </div>

      <div class="flex justify-end gap-2 border-t border-border px-4 py-3">
        <button
          v-if="status?.installed"
          class="rounded border border-border px-3 py-1.5 text-sm hover:bg-muted disabled:opacity-50"
          :disabled="busy"
          @click="uninstall"
        >
          Uninstall
        </button>
        <button
          class="rounded bg-primary px-3 py-1.5 text-sm text-primary-foreground hover:opacity-90 disabled:opacity-50"
          :disabled="busy || !status?.supported"
          @click="install"
        >
          {{ busy ? 'Working…' : status?.installed ? 'Reinstall' : 'Install' }}
        </button>
      </div>
    </div>
  </div>
</template>
