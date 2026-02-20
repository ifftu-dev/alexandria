<script setup lang="ts">
import { useTheme } from '@/composables/useTheme'

interface Props {
  sidebarCollapsed: boolean
}

defineProps<Props>()

const { theme, toggleTheme } = useTheme()
</script>

<template>
  <header class="flex items-center justify-between h-14 px-6 border-b border-[rgb(var(--color-border))] bg-[rgb(var(--color-card))]">
    <div class="text-sm font-medium text-[rgb(var(--color-foreground))]">
      Alexandria Node
    </div>

    <div class="flex items-center gap-3">
      <!-- Connection status (placeholder for P2P in Phase 3) -->
      <div class="flex items-center gap-1.5 text-xs text-[rgb(var(--color-muted-foreground))]">
        <span class="w-2 h-2 rounded-full bg-[rgb(var(--color-success))]" />
        Local
      </div>

      <!-- Theme toggle -->
      <button
        class="p-1.5 rounded-md text-sm text-[rgb(var(--color-muted-foreground))] hover:bg-[rgb(var(--color-muted)/0.5)]"
        :title="`Theme: ${theme}`"
        @click="toggleTheme"
      >
        {{ theme === 'dark' ? '\u263E' : theme === 'light' ? '\u2600' : '\u25D0' }}
      </button>
    </div>
  </header>
</template>
