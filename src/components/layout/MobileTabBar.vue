<script setup lang="ts">
import { ref, watch } from 'vue'
import { useRouter, useRoute } from 'vue-router'
import { useTheme } from '@/composables/useTheme'
import { useAuth } from '@/composables/useAuth'

const router = useRouter()
const route = useRoute()
const { theme, toggleTheme } = useTheme()
const { lockVault, displayName } = useAuth()

const moreOpen = ref(false)

// Close drawer on route change
watch(() => route.path, () => { moreOpen.value = false })

interface Tab {
  label: string
  path: string
  icon: string
}

const primaryTabs: Tab[] = [
  {
    label: 'Home',
    path: '/home',
    icon: 'M3 12l2-2m0 0l7-7 7 7M5 10v10a1 1 0 001 1h3m10-11l2 2m-2-2v10a1 1 0 01-1 1h-3m-6 0a1 1 0 001-1v-4a1 1 0 011-1h2a1 1 0 011 1v4a1 1 0 001 1m-6 0h6',
  },
  {
    label: 'Courses',
    path: '/courses',
    icon: 'M12 6.253v13m0-13C10.832 5.477 9.246 5 7.5 5S4.168 5.477 3 6.253v13C4.168 18.477 5.754 18 7.5 18s3.332.477 4.5 1.253m0-13C13.168 5.477 14.754 5 16.5 5c1.747 0 3.332.477 4.5 1.253v13C19.832 18.477 18.247 18 16.5 18c-1.746 0-3.332.477-4.5 1.253',
  },
  {
    label: 'Skills',
    path: '/skills',
    icon: 'M9 12l2 2 4-4M7.835 4.697a3.42 3.42 0 001.946-.806 3.42 3.42 0 014.438 0 3.42 3.42 0 001.946.806 3.42 3.42 0 013.138 3.138 3.42 3.42 0 00.806 1.946 3.42 3.42 0 010 4.438 3.42 3.42 0 00-.806 1.946 3.42 3.42 0 01-3.138 3.138 3.42 3.42 0 00-1.946.806 3.42 3.42 0 01-4.438 0 3.42 3.42 0 00-1.946-.806 3.42 3.42 0 01-3.138-3.138 3.42 3.42 0 00-.806-1.946 3.42 3.42 0 010-4.438 3.42 3.42 0 00.806-1.946 3.42 3.42 0 013.138-3.138z',
  },
  {
    label: 'Govern',
    path: '/governance',
    icon: 'M3 6l3 1m0 0l-3 9a5.002 5.002 0 006.001 0M6 7l3 9M6 7l6-2m6 2l3-1m-3 1l-3 9a5.002 5.002 0 006.001 0M18 7l3 9m-3-9l-6-2m0-2v2m0 16V5m0 16H9m3 0h3',
  },
]

interface DrawerSection {
  title: string
  items: Tab[]
}

const drawerSections: DrawerSection[] = [
  {
    title: 'Dashboard',
    items: [
      { label: 'My Courses', path: '/dashboard/courses', icon: 'M5 8h14M5 8a2 2 0 110-4h14a2 2 0 110 4M5 8v10a2 2 0 002 2h10a2 2 0 002-2V8m-9 4h4' },
      { label: 'Credentials', path: '/dashboard/credentials', icon: 'M9 12l2 2 4-4m5.618-4.016A11.955 11.955 0 0112 2.944a11.955 11.955 0 01-8.618 3.04A12.02 12.02 0 003 9c0 5.591 3.824 10.29 9 11.622 5.176-1.332 9-6.03 9-11.622 0-1.042-.133-2.052-.382-3.016z' },
      { label: 'Reputation', path: '/dashboard/reputation', icon: 'M13 7h8m0 0v8m0-8l-8 8-4-4-6 6' },
      { label: 'Sentinel', path: '/dashboard/sentinel', icon: 'M9 12.75L11.25 15 15 9.75m-3-7.036A11.959 11.959 0 013.598 6 11.99 11.99 0 003 9.749c0 5.592 3.824 10.29 9 11.623 5.176-1.332 9-6.03 9-11.622 0-1.31-.21-2.571-.598-3.751h-.152c-3.196 0-6.1-1.248-8.25-3.285z' },
    ],
  },
  {
    title: 'Node',
    items: [
      { label: 'Network', path: '/dashboard/network', icon: 'M21 12a9 9 0 01-9 9m9-9a9 9 0 00-9-9m9 9H3m9 9a9 9 0 01-9-9m9 9c1.657 0 3-4.03 3-9s-1.343-9-3-9m0 18c-1.657 0-3-4.03-3-9s1.343-9 3-9m-9 9a9 9 0 019-9' },
      { label: 'Sync', path: '/dashboard/sync', icon: 'M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15' },
      { label: 'Settings', path: '/dashboard/settings', icon: 'M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.066 2.573c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.573 1.066c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.066-2.573c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z' },
    ],
  },
]

const themeIcons: Record<string, string> = {
  light: 'M12 3v1m0 16v1m9-9h-1M4 12H3m15.364 6.364l-.707-.707M6.343 6.343l-.707-.707m12.728 0l-.707.707M6.343 17.657l-.707.707M16 12a4 4 0 11-8 0 4 4 0 018 0z',
  dark: 'M20.354 15.354A9 9 0 018.646 3.646 9.003 9.003 0 0012 21a9.003 9.003 0 008.354-5.646z',
  system: 'M9.75 17L9 20l-1 1h8l-1-1-.75-3M3 13h18M5 17h14a2 2 0 002-2V5a2 2 0 00-2-2H5a2 2 0 00-2 2v10a2 2 0 002 2z',
}

function isActive(path: string) {
  return route.path === path || route.path.startsWith(path + '/')
}

function isMoreActive() {
  return drawerSections.some(s => s.items.some(i => isActive(i.path)))
}

function navigate(path: string) {
  router.push(path)
  moreOpen.value = false
}

async function signOut() {
  try { await lockVault() } catch (e) { console.warn('lock failed:', e) }
  moreOpen.value = false
  router.replace('/unlock')
}
</script>

<template>
  <div class="md:hidden">
  <!-- Backdrop (Mark 2 style: bg-black/50) -->
  <Transition name="fade">
    <div
      v-if="moreOpen"
      class="fixed inset-0 z-[60] bg-black/50"
      @click="moreOpen = false"
    />
  </Transition>

  <!-- Slide-up drawer -->
  <Transition name="slide-up">
    <div
      v-if="moreOpen"
      class="fixed bottom-0 left-0 right-0 z-[70] bg-card shadow-lg safe-area-bottom"
    >
      <!-- Drag handle -->
      <div class="flex justify-center pt-3 pb-1">
        <div class="w-10 h-1 rounded-full bg-border" />
      </div>

      <!-- Drawer content -->
      <div class="px-4 pb-16 max-h-[70vh] overflow-y-auto">
        <div v-for="section in drawerSections" :key="section.title" class="mb-4 last:mb-0">
          <!-- Section header (Mark 2 style: larger, full opacity) -->
          <div class="px-3 pb-1.5 text-[0.8125rem] font-semibold tracking-wider uppercase text-muted-foreground">
            {{ section.title }}
          </div>
          <button
            v-for="item in section.items"
            :key="item.path"
            class="relative flex items-center w-full gap-3 rounded-lg px-3 py-2.5 text-sm font-medium transition-colors"
            :class="isActive(item.path)
              ? 'text-primary'
              : 'text-foreground active:bg-muted'"
            @click="navigate(item.path)"
          >
            <!-- Active indicator — left bar (Mark 2 style) -->
            <span
              v-if="isActive(item.path)"
              class="absolute left-0 top-1 bottom-1 w-0.5 rounded-full bg-primary"
            />
            <svg class="h-5 w-5 shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
              <path stroke-linecap="round" stroke-linejoin="round" :d="item.icon" />
            </svg>
            {{ item.label }}
          </button>
        </div>

        <!-- Divider -->
        <div class="my-3 h-px bg-border" />

        <!-- Theme toggle -->
        <button
          class="flex items-center w-full gap-3 rounded-lg px-3 py-2.5 text-sm font-medium text-foreground active:bg-muted transition-colors"
          @click="toggleTheme"
        >
          <svg class="h-5 w-5 shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
            <path stroke-linecap="round" stroke-linejoin="round" :d="themeIcons[theme]" />
          </svg>
          Theme: {{ theme.charAt(0).toUpperCase() + theme.slice(1) }}
        </button>

        <!-- User info + Lock -->
        <div class="mt-2 flex items-center justify-between rounded-lg px-3 py-2.5">
          <div class="flex items-center gap-3 min-w-0">
            <div class="w-8 h-8 rounded-full bg-gradient-to-br from-primary to-accent flex items-center justify-center text-white text-xs font-bold shrink-0">
              {{ displayName ? displayName.charAt(0).toUpperCase() : 'A' }}
            </div>
            <span class="text-sm font-medium text-foreground truncate">
              {{ displayName ?? 'Anonymous' }}
            </span>
          </div>
          <button
            class="flex items-center gap-1.5 rounded-md px-3 py-1.5 text-xs font-medium text-error bg-error/10 active:bg-error/20 transition-colors"
            @click="signOut"
          >
            <svg class="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
              <path stroke-linecap="round" stroke-linejoin="round" d="M15.75 9V5.25A2.25 2.25 0 0013.5 3h-6a2.25 2.25 0 00-2.25 2.25v13.5A2.25 2.25 0 007.5 21h6a2.25 2.25 0 002.25-2.25V15m3 0l3-3m0 0l-3-3m3 3H9" />
            </svg>
            Lock
          </button>
        </div>
      </div>
    </div>
  </Transition>

  <!-- Tab bar -->
  <nav
    class="fixed bottom-0 left-0 right-0 z-[70] flex items-stretch border-t border-border bg-card safe-area-bottom"
  >
    <button
      v-for="tab in primaryTabs"
      :key="tab.path"
      class="flex flex-1 flex-col items-center justify-center gap-0.5 pt-2 pb-1 transition-colors"
      :class="isActive(tab.path)
        ? 'text-primary'
        : 'text-muted-foreground'"
      @click="navigate(tab.path)"
    >
      <svg class="w-5 h-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" :stroke-width="isActive(tab.path) ? 2.25 : 1.75">
        <path stroke-linecap="round" stroke-linejoin="round" :d="tab.icon" />
      </svg>
      <span class="text-[0.6rem] font-medium leading-tight">{{ tab.label }}</span>
    </button>

    <!-- More button -->
    <button
      class="flex flex-1 flex-col items-center justify-center gap-0.5 pt-2 pb-1 transition-colors"
      :class="moreOpen || isMoreActive()
        ? 'text-primary'
        : 'text-muted-foreground'"
      @click="moreOpen = !moreOpen"
    >
      <svg class="w-5 h-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" :stroke-width="moreOpen || isMoreActive() ? 2.25 : 1.75">
        <path stroke-linecap="round" stroke-linejoin="round" d="M3.75 6.75h16.5M3.75 12h16.5m-16.5 5.25h16.5" />
      </svg>
      <span class="text-[0.6rem] font-medium leading-tight">More</span>
    </button>
  </nav>
  </div>
</template>

<style scoped>
.fade-enter-active, .fade-leave-active {
  transition: opacity 300ms ease;
}
.fade-enter-from, .fade-leave-to {
  opacity: 0;
}

.slide-up-enter-active, .slide-up-leave-active {
  transition: transform 300ms cubic-bezier(0.4, 0, 0.2, 1);
}
.slide-up-enter-from, .slide-up-leave-to {
  transform: translateY(100%);
}
</style>
