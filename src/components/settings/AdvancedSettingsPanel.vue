<script setup lang="ts">
import { computed, onMounted } from 'vue'

import { AppButton, AppInput } from '@/components/ui'
import { useSettings, type SettingEntry } from '@/composables/useSettings'

// Render every registered setting, grouped by category. The registry
// drives the UI — when a new key is added to `settings::registry::keys`
// in Rust, it appears here automatically with the right widget.

const { entries, ready, initialize, setSetting, resetSetting } = useSettings()

onMounted(async () => {
  await initialize()
})

const grouped = computed<Record<string, SettingEntry[]>>(() => {
  const out: Record<string, SettingEntry[]> = {}
  for (const e of entries.value) {
    if (!out[e.category]) out[e.category] = []
    out[e.category]!.push(e)
  }
  return out
})

function asBool(entry: SettingEntry): boolean {
  return entry.current_value === 'true' || entry.current_value === '1'
}

async function setBool(entry: SettingEntry, value: boolean) {
  await setSetting(entry.key, value ? 'true' : 'false')
}

async function setString(entry: SettingEntry, value: string) {
  await setSetting(entry.key, value)
}
</script>

<template>
  <div v-if="!ready" class="p-6 text-sm text-muted-foreground">{{ $t('settings.advanced.loading') }}</div>
  <div v-else class="space-y-6">
    <div
      v-for="(group, category) in grouped"
      :key="category"
      class="card p-4"
    >
      <h3 class="text-sm font-semibold text-foreground mb-3">{{ category }}</h3>
      <div class="space-y-4">
        <div
          v-for="entry in group"
          :key="entry.key"
          class="flex items-start justify-between gap-4"
        >
          <div class="flex-1 min-w-0">
            <div class="flex items-center gap-2">
              <span class="text-sm font-medium text-foreground">{{ entry.label }}</span>
              <span
                class="text-[10px] uppercase tracking-wide rounded px-1.5 py-0.5"
                :class="entry.scope === 'sync'
                  ? 'bg-primary/10 text-primary'
                  : 'bg-muted text-muted-foreground'"
                :title="entry.scope === 'sync'
                  ? $t('settings.advanced.syncedTooltip')
                  : $t('settings.advanced.deviceTooltip')"
              >
                {{ entry.scope }}
              </span>
              <code class="text-[10px] text-muted-foreground/70">{{ entry.key }}</code>
            </div>
            <p class="text-xs text-muted-foreground mt-0.5">{{ entry.description }}</p>
          </div>

          <div class="shrink-0 flex items-center gap-2">
            <!-- bool -->
            <label v-if="entry.kind === 'bool'" class="inline-flex items-center cursor-pointer">
              <input
                type="checkbox"
                :checked="asBool(entry)"
                @change="setBool(entry, ($event.target as HTMLInputElement).checked)"
              >
            </label>

            <!-- string -->
            <AppInput
              v-else-if="entry.kind === 'string'"
              :model-value="entry.current_value"
              class="w-48"
              @update:model-value="(v: string) => setString(entry, v)"
            />

            <!-- int / float -->
            <AppInput
              v-else-if="entry.kind === 'int' || entry.kind === 'float'"
              :model-value="entry.current_value"
              type="number"
              class="w-32"
              @update:model-value="(v: string) => setString(entry, v)"
            />

            <!-- json (read-only summary; advanced users edit via CLI) -->
            <code
              v-else
              class="text-xs text-muted-foreground bg-muted/40 px-2 py-1 rounded max-w-[14rem] truncate"
              :title="entry.current_value"
            >
              {{ entry.current_value.slice(0, 40) }}
            </code>

            <AppButton
              v-if="!entry.is_default"
              variant="ghost"
              size="xs"
              @click="resetSetting(entry.key)"
            >
              {{ $t('settings.advanced.reset') }}
            </AppButton>
          </div>
        </div>
      </div>
    </div>

    <p class="text-xs text-muted-foreground px-1">
      {{ $t('settings.advanced.syncNote') }}
    </p>
  </div>
</template>
