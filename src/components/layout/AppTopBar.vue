<script setup lang="ts">
import { onMounted } from 'vue'
import { useRoute } from 'vue-router'
import { useTheme } from '@/composables/useTheme'
import { useP2P } from '@/composables/useP2P'
import { useAuth } from '@/composables/useAuth'

interface Props {
  sidebarCollapsed: boolean
}

defineProps<Props>()

const route = useRoute()
const { theme, toggleTheme } = useTheme()
const { status: p2pStatus, startPolling } = useP2P()
const { displayName } = useAuth()

onMounted(() => {
  startPolling(15000)
})

// Simple breadcrumb from route path
function breadcrumb(): string {
  const segments = route.path.split('/').filter(Boolean)
  if (segments.length === 0) return 'Home'
  return segments.map(s => s.charAt(0).toUpperCase() + s.slice(1)).join(' / ')
}
</script>

<template>
  <header class="flex items-center justify-between h-12 md:h-14 px-3 md:px-6 border-b border-border bg-card">
    <div class="text-sm text-muted-foreground truncate">
      {{ breadcrumb() }}
    </div>

    <div class="flex items-center gap-3 md:gap-4">
      <!-- P2P status — hidden on mobile -->
      <div class="hidden md:flex items-center gap-1.5 text-xs text-muted-foreground">
        <span
          class="w-2 h-2 rounded-full"
          :class="p2pStatus?.is_running
            ? 'bg-success'
            : p2pStatus != null ? 'bg-muted-foreground/40' : 'bg-amber-500 animate-pulse'"
        />
        {{ p2pStatus?.is_running ? `${p2pStatus.connected_peers} peer${p2pStatus.connected_peers !== 1 ? 's' : ''}` : p2pStatus != null ? 'Offline' : 'Starting...' }}
      </div>

      <!-- Theme toggle -->
      <button
        class="p-1.5 rounded-md text-muted-foreground hover:bg-muted/50 transition-colors"
        :title="`Theme: ${theme}`"
        @click="toggleTheme"
      >
        <svg class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
          <path v-if="theme === 'dark'" stroke-linecap="round" stroke-linejoin="round" d="M20.354 15.354A9 9 0 018.646 3.646 9.003 9.003 0 0012 21a9.003 9.003 0 008.354-5.646z" />
          <path v-else-if="theme === 'light'" stroke-linecap="round" stroke-linejoin="round" d="M12 3v1m0 16v1m9-9h-1M4 12H3m15.364 6.364l-.707-.707M6.343 6.343l-.707-.707m12.728 0l-.707.707M6.343 17.657l-.707.707M16 12a4 4 0 11-8 0 4 4 0 018 0z" />
          <path v-else stroke-linecap="round" stroke-linejoin="round" d="M9.75 17L9 20l-1 1h8l-1-1-.75-3M3 13h18M5 17h14a2 2 0 002-2V5a2 2 0 00-2-2H5a2 2 0 00-2 2v10a2 2 0 002 2z" />
        </svg>
      </button>

      <!-- User avatar -->
      <div
        class="w-7 h-7 rounded-full bg-gradient-to-br from-primary to-accent flex items-center justify-center text-white text-xs font-bold"
        :title="displayName ?? 'Profile'"
      >
        {{ displayName ? displayName.charAt(0).toUpperCase() : 'A' }}
      </div>
    </div>
  </header>
</template>
