<script setup lang="ts">
import { onMounted } from 'vue'
import { useRouter, useRoute } from 'vue-router'
import { useP2P } from '@/composables/useP2P'
import { useAuth } from '@/composables/useAuth'
import { useTheme } from '@/composables/useTheme'

interface Props {
  collapsed: boolean
}

interface NavItem {
  label: string
  path: string
  section?: string
}

defineProps<Props>()
const emit = defineEmits<{ toggle: [] }>()
const router = useRouter()
const route = useRoute()
const { status: p2pStatus, startPolling } = useP2P()
const { lockVault, displayName, stakeAddress } = useAuth()
const { theme, toggleTheme } = useTheme()

onMounted(() => {
  // Start polling P2P status so the sidebar reflects node state.
  // The singleton guard in useP2P prevents duplicate intervals if
  // AppTopBar already started polling.
  startPolling(15000)
})

const themeIcon: Record<string, string> = {
  light: 'M12 3v1m0 16v1m9-9h-1M4 12H3m15.364 6.364l-.707-.707M6.343 6.343l-.707-.707m12.728 0l-.707.707M6.343 17.657l-.707.707M16 12a4 4 0 11-8 0 4 4 0 018 0z',
  dark: 'M20.354 15.354A9 9 0 018.646 3.646 9.003 9.003 0 0012 21a9.003 9.003 0 008.354-5.646z',
  system: 'M9.75 17L9 20l-1 1h8l-1-1-.75-3M3 13h18M5 17h14a2 2 0 002-2V5a2 2 0 00-2-2H5a2 2 0 00-2 2v10a2 2 0 002 2z',
}
const themeLabel: Record<string, string> = {
  light: 'Light',
  dark: 'Dark',
  system: 'System',
}

async function signOut() {
  try {
    await lockVault()
  } catch (e) {
    console.warn('lock failed:', e)
  }
  router.replace('/unlock')
}

const navSections: { title: string; items: NavItem[] }[] = [
  {
    title: 'Main',
    items: [
      { label: 'Home', path: '/home' },
      { label: 'Courses', path: '/courses' },
      { label: 'Skills', path: '/skills' },
      { label: 'Governance', path: '/governance' },
    ],
  },
  {
    title: 'Dashboard',
    items: [
      { label: 'My Courses', path: '/dashboard/courses' },
      { label: 'Credentials', path: '/dashboard/credentials' },
      { label: 'Reputation', path: '/dashboard/reputation' },
      { label: 'Sentinel', path: '/dashboard/sentinel' },
    ],
  },
  {
    title: 'Node',
    items: [
      { label: 'Network', path: '/dashboard/network' },
      { label: 'Sync', path: '/dashboard/sync' },
      { label: 'Settings', path: '/dashboard/settings' },
    ],
  },
]

const icons: Record<string, string> = {
  '/home': 'M3 12l2-2m0 0l7-7 7 7M5 10v10a1 1 0 001 1h3m10-11l2 2m-2-2v10a1 1 0 01-1 1h-3m-6 0a1 1 0 001-1v-4a1 1 0 011-1h2a1 1 0 011 1v4a1 1 0 001 1m-6 0h6',
  '/courses': 'M12 6.253v13m0-13C10.832 5.477 9.246 5 7.5 5S4.168 5.477 3 6.253v13C4.168 18.477 5.754 18 7.5 18s3.332.477 4.5 1.253m0-13C13.168 5.477 14.754 5 16.5 5c1.747 0 3.332.477 4.5 1.253v13C19.832 18.477 18.247 18 16.5 18c-1.746 0-3.332.477-4.5 1.253',
  '/skills': 'M9 12l2 2 4-4M7.835 4.697a3.42 3.42 0 001.946-.806 3.42 3.42 0 014.438 0 3.42 3.42 0 001.946.806 3.42 3.42 0 013.138 3.138 3.42 3.42 0 00.806 1.946 3.42 3.42 0 010 4.438 3.42 3.42 0 00-.806 1.946 3.42 3.42 0 01-3.138 3.138 3.42 3.42 0 00-1.946.806 3.42 3.42 0 01-4.438 0 3.42 3.42 0 00-1.946-.806 3.42 3.42 0 01-3.138-3.138 3.42 3.42 0 00-.806-1.946 3.42 3.42 0 010-4.438 3.42 3.42 0 00.806-1.946 3.42 3.42 0 013.138-3.138z',
  '/governance': 'M3 6l3 1m0 0l-3 9a5.002 5.002 0 006.001 0M6 7l3 9M6 7l6-2m6 2l3-1m-3 1l-3 9a5.002 5.002 0 006.001 0M18 7l3 9m-3-9l-6-2m0-2v2m0 16V5m0 16H9m3 0h3',
  '/dashboard/courses': 'M5 8h14M5 8a2 2 0 110-4h14a2 2 0 110 4M5 8v10a2 2 0 002 2h10a2 2 0 002-2V8m-9 4h4',
  '/dashboard/credentials': 'M9 12l2 2 4-4m5.618-4.016A11.955 11.955 0 0112 2.944a11.955 11.955 0 01-8.618 3.04A12.02 12.02 0 003 9c0 5.591 3.824 10.29 9 11.622 5.176-1.332 9-6.03 9-11.622 0-1.042-.133-2.052-.382-3.016z',
  '/dashboard/reputation': 'M13 7h8m0 0v8m0-8l-8 8-4-4-6 6',
  '/dashboard/sentinel': 'M9 12.75L11.25 15 15 9.75m-3-7.036A11.959 11.959 0 013.598 6 11.99 11.99 0 003 9.749c0 5.592 3.824 10.29 9 11.623 5.176-1.332 9-6.03 9-11.622 0-1.31-.21-2.571-.598-3.751h-.152c-3.196 0-6.1-1.248-8.25-3.285z',
  '/dashboard/network': 'M21 12a9 9 0 01-9 9m9-9a9 9 0 00-9-9m9 9H3m9 9a9 9 0 01-9-9m9 9c1.657 0 3-4.03 3-9s-1.343-9-3-9m0 18c-1.657 0-3-4.03-3-9s1.343-9 3-9m-9 9a9 9 0 019-9',
  '/dashboard/sync': 'M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15',
  '/dashboard/settings': 'M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.066 2.573c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.573 1.066c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.066-2.573c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z',
}

function isActive(path: string) {
  return route.path === path || route.path.startsWith(path + '/')
}

function navigate(path: string) {
  router.push(path)
}
</script>

<template>
  <aside
    class="flex flex-col border-r border-border bg-card transition-all duration-200 select-none"
    :class="collapsed ? 'w-16' : 'w-56'"
  >
    <!-- Logo -->
    <div class="flex items-center gap-2 h-14 px-4 border-b border-border">
      <svg class="w-6 h-6 text-primary shrink-0" viewBox="0 0 32 32" fill="none">
        <path d="M16 2L4 8v16l12 6 12-6V8L16 2z" stroke="currentColor" stroke-width="2" fill="none" />
        <path d="M16 8v16M8 12l8 4 8-4" stroke="currentColor" stroke-width="2" />
      </svg>
      <span v-if="!collapsed" class="font-semibold text-sm tracking-tight">Alexandria</span>
    </div>

    <!-- Navigation -->
    <nav class="flex-1 py-2 px-2 overflow-y-auto">
      <div v-for="section in navSections" :key="section.title" class="mb-3">
        <div
          v-if="!collapsed"
          class="px-2.5 py-1 text-[0.65rem] font-semibold tracking-wider uppercase text-muted-foreground/60"
        >
          {{ section.title }}
        </div>
        <div v-else class="h-px bg-border/50 mx-2 my-1" />

        <button
          v-for="item in section.items"
          :key="item.path"
          class="flex items-center w-full rounded-md px-2.5 py-2 text-sm transition-colors"
          :class="isActive(item.path)
            ? 'bg-primary/10 text-primary font-medium'
            : 'text-muted-foreground hover:bg-muted/50 hover:text-foreground'"
          :title="collapsed ? item.label : undefined"
          @click="navigate(item.path)"
        >
          <svg class="w-4 h-4 shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.75">
            <path stroke-linecap="round" stroke-linejoin="round" :d="icons[item.path] ?? ''" />
          </svg>
          <span v-if="!collapsed" class="ml-2.5 truncate">{{ item.label }}</span>
        </button>
      </div>
    </nav>

    <!-- P2P Status -->
    <div class="px-3 py-2 border-t border-border">
      <div class="flex items-center gap-1.5">
        <span
          class="w-2 h-2 rounded-full shrink-0"
          :class="p2pStatus?.is_running
            ? 'bg-success'
            : p2pStatus != null ? 'bg-muted-foreground/40' : 'bg-amber-500 animate-pulse'"
        />
        <span v-if="!collapsed" class="text-xs text-muted-foreground">
          {{ p2pStatus?.is_running ? `${p2pStatus.connected_peers} peers` : p2pStatus != null ? 'Offline' : 'Starting...' }}
        </span>
      </div>
    </div>

    <!-- Theme toggle -->
    <div class="px-2 py-1 border-t border-border">
      <button
        class="flex items-center w-full rounded-md px-2.5 py-2 text-sm text-muted-foreground hover:bg-muted/50 hover:text-foreground transition-colors"
        :title="collapsed ? `Theme: ${themeLabel[theme]}` : undefined"
        @click="toggleTheme"
      >
        <svg class="w-4 h-4 shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.75">
          <path stroke-linecap="round" stroke-linejoin="round" :d="themeIcon[theme]" />
        </svg>
        <span v-if="!collapsed" class="ml-2.5">{{ themeLabel[theme] }}</span>
      </button>
    </div>

    <!-- User / Lock -->
    <div class="px-2 py-2 border-t border-border">
      <div v-if="!collapsed" class="px-2.5 mb-1.5 truncate">
        <p class="text-xs font-medium text-foreground truncate">
          {{ displayName ?? 'Anonymous' }}
        </p>
        <p class="text-[0.6rem] text-muted-foreground truncate">
          {{ stakeAddress ? stakeAddress.slice(0, 20) + '...' : '' }}
        </p>
      </div>
      <button
        class="flex items-center w-full rounded-md px-2.5 py-2 text-sm text-muted-foreground hover:bg-error/10 hover:text-error transition-colors"
        :title="collapsed ? 'Lock & Sign Out' : undefined"
        @click="signOut"
      >
        <svg class="w-4 h-4 shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.75">
          <path stroke-linecap="round" stroke-linejoin="round" d="M15.75 9V5.25A2.25 2.25 0 0013.5 3h-6a2.25 2.25 0 00-2.25 2.25v13.5A2.25 2.25 0 007.5 21h6a2.25 2.25 0 002.25-2.25V15m3 0l3-3m0 0l-3-3m3 3H9" />
        </svg>
        <span v-if="!collapsed" class="ml-2.5">Lock & Sign Out</span>
      </button>
    </div>

    <!-- Collapse toggle -->
    <div class="p-2 border-t border-border">
      <button
        class="flex items-center justify-center w-full rounded-md p-2 text-xs text-muted-foreground hover:bg-muted/50"
        @click="emit('toggle')"
      >
        <svg class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
          <path stroke-linecap="round" stroke-linejoin="round" :d="collapsed ? 'M13 5l7 7-7 7' : 'M11 19l-7-7 7-7'" />
        </svg>
      </button>
    </div>
  </aside>
</template>
