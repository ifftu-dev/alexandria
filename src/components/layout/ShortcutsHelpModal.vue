<script setup lang="ts">
/**
 * The keyboard-shortcut cheat sheet, opened with Cmd/Ctrl + ?.
 *
 * Rendered from the live shortcut registry rather than a hand-written list,
 * so it shows the user's *current* bindings — including any they rebound —
 * and cannot drift out of date when a shortcut is added or changed.
 */

import { computed } from 'vue'
import { useI18n } from 'vue-i18n'

import { AppModal } from '@/components/ui'
import { shortcutName, useKeyboardShortcuts } from '@/composables/useKeyboardShortcuts'

defineProps<{ open: boolean }>()
defineEmits<{ close: [] }>()

const { t } = useI18n()
const { shortcuts, formatCombo } = useKeyboardShortcuts()

const rows = computed(() =>
  Object.values(shortcuts).map((def) => ({
    id: def.id,
    name: shortcutName(def),
    combo: formatCombo(def.keys),
  })),
)
</script>

<template>
  <AppModal
    :open="open"
    :title="t('settings.personalization.shortcutsModalTitle')"
    max-width="30rem"
    @close="$emit('close')"
  >
    <div class="space-y-1">
      <div
        v-for="row in rows"
        :key="row.id"
        class="flex items-center justify-between gap-4 rounded-lg px-3 py-2 text-sm"
      >
        <span class="min-w-0 flex-1 truncate text-foreground">{{ row.name }}</span>
        <kbd
          class="shrink-0 rounded border border-border bg-muted px-2 py-1 font-mono text-xs text-foreground"
          >{{ row.combo }}</kbd
        >
      </div>
      <p class="px-3 pt-3 text-xs text-muted-foreground">
        {{ t('settings.personalization.shortcutsModalHint') }}
      </p>
    </div>
  </AppModal>
</template>
