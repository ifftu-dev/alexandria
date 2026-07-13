<script lang="ts">
// Shared types (exported for the parent that orchestrates install + enroll).
export interface RequiredPlugin {
  plugin_cid: string
  name: string
  icon_path: string | null
  scope: string
  installed: boolean
}

export interface PluginProgress {
  step: string // "installing" | "done" | "failed"
  index: number
  total: number
}
</script>

<script setup lang="ts">
// Enrollment pre-flight dialog. Shows which plugins a course requires and which
// already exist on the machine. On Continue the parent installs the missing
// ones (precompiling graders) and drives the per-plugin progress shown here;
// on Cancel enrollment is aborted. Presentational only — the parent owns the
// install + enroll orchestration and passes `installing` / `progress` / `error`.
import { computed } from 'vue'
import { AppModal, AppButton, AppSpinner } from '@/components/ui'

const props = defineProps<{
  open: boolean
  plugins: RequiredPlugin[]
  installing: boolean
  progress: Record<string, PluginProgress>
  error: string | null
}>()

const emit = defineEmits<{ continue: []; cancel: [] }>()

const missingCount = computed(() => props.plugins.filter(p => !p.installed).length)

// A plugin's live status: already installed, or its in-flight install step.
function statusOf(p: RequiredPlugin): 'installed' | 'installing' | 'done' | 'failed' | 'pending' {
  const step = props.progress[p.plugin_cid]?.step
  if (step === 'failed') return 'failed'
  if (step === 'installing') return 'installing'
  if (step === 'done' || p.installed) return 'installed'
  if (props.installing) return 'pending'
  return p.installed ? 'installed' : 'pending'
}

// Overall progress fraction across the plugins being installed.
const progressPct = computed(() => {
  const total = props.plugins.length
  if (total === 0) return 0
  const done = props.plugins.filter(
    p => p.installed || props.progress[p.plugin_cid]?.step === 'done',
  ).length
  return Math.round((done / total) * 100)
})
</script>

<template>
  <AppModal :open="open" :title="$t('courses.enrollPlugins.title')" @close="!installing && emit('cancel')">
    <p class="text-sm text-muted-foreground mb-4">
      {{ missingCount > 0
        ? $t('courses.enrollPlugins.bodyInstall', { count: missingCount })
        : $t('courses.enrollPlugins.bodyReady') }}
    </p>

    <ul class="flex flex-col gap-2">
      <li
        v-for="p in plugins"
        :key="p.plugin_cid"
        class="flex items-center gap-3 rounded-lg border border-border p-3"
      >
        <svg class="w-5 h-5 shrink-0 text-muted-foreground" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.6">
          <path stroke-linecap="round" stroke-linejoin="round" d="M11 4a2 2 0 114 0v1a1 1 0 001 1h3a1 1 0 011 1v3a1 1 0 01-1 1h-1a2 2 0 100 4h1a1 1 0 011 1v3a1 1 0 01-1 1h-3a1 1 0 01-1-1v-1a2 2 0 10-4 0v1a1 1 0 01-1 1H6a1 1 0 01-1-1v-3a1 1 0 011-1h1a2 2 0 100-4H6a1 1 0 01-1-1V8a1 1 0 011-1h3a1 1 0 001-1V4z" />
        </svg>

        <div class="min-w-0 flex-1">
          <div class="text-sm font-medium truncate">{{ p.name }}</div>
        </div>

        <!-- Status pill -->
        <span
          v-if="statusOf(p) === 'installed'"
          class="badge-success text-xs px-2 py-0.5 rounded-full"
        >{{ $t('courses.enrollPlugins.status.ready') }}</span>
        <span
          v-else-if="statusOf(p) === 'installing'"
          class="inline-flex items-center gap-1.5 text-xs text-muted-foreground"
        >
          <AppSpinner class="w-3.5 h-3.5" />
          {{ $t('courses.enrollPlugins.status.installing') }}
        </span>
        <span
          v-else-if="statusOf(p) === 'failed'"
          class="badge-error text-xs px-2 py-0.5 rounded-full"
        >{{ $t('courses.enrollPlugins.status.failed') }}</span>
        <span
          v-else
          class="text-xs px-2 py-0.5 rounded-full bg-muted/60 text-muted-foreground"
        >{{ $t('courses.enrollPlugins.status.willInstall') }}</span>
      </li>
    </ul>

    <!-- Overall progress bar (only while installing) -->
    <div v-if="installing" class="mt-4">
      <div class="h-1.5 w-full rounded-full bg-muted/50 overflow-hidden">
        <div
          class="h-full rounded-full bg-primary transition-all duration-300"
          :style="{ width: `${progressPct}%` }"
        />
      </div>
    </div>

    <p v-if="error" class="mt-3 text-sm badge-error px-3 py-2 rounded-md">
      {{ error }}
    </p>

    <template #footer>
      <div class="flex justify-end gap-2">
        <AppButton variant="ghost" :disabled="installing" @click="emit('cancel')">
          {{ $t('common.actions.cancel') }}
        </AppButton>
        <AppButton :loading="installing" @click="emit('continue')">
          {{ missingCount > 0
            ? $t('courses.enrollPlugins.continueInstall')
            : $t('courses.enrollPlugins.continue') }}
        </AppButton>
      </div>
    </template>
  </AppModal>
</template>
