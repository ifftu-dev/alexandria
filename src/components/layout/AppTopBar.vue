<script setup lang="ts">
import { ref, onMounted, onUnmounted } from 'vue'
import { useRouter, useRoute } from 'vue-router'
import { useTheme } from '@/composables/useTheme'
import { useP2P } from '@/composables/useP2P'
import { useAuth } from '@/composables/useAuth'

defineProps<{ sidebarCollapsed: boolean }>()
const emit = defineEmits<{ toggleSidebar: [] }>()

const router = useRouter()
const route = useRoute()
const { theme, setTheme } = useTheme()
const { status: p2pStatus, startPolling } = useP2P()
const { displayName, lockVault } = useAuth()
const isMac = typeof navigator !== 'undefined' && /Mac/.test(navigator.userAgent)
const isMobilePlatform = typeof navigator !== 'undefined' && /iPhone|iPad|iPod|Android/i.test(navigator.userAgent)
const canGoBack = ref(false)
const canGoForward = ref(false)

function syncNavButtons() {
  if (typeof window === 'undefined') return
  const state = (window.history.state ?? {}) as { back?: string | null; forward?: string | null }
  canGoBack.value = Boolean(state.back)
  canGoForward.value = Boolean(state.forward)
}

// --- Search ---
const searchQuery = ref('')
const searchFocused = ref(false)
const searchInput = ref<HTMLInputElement | null>(null)

function onSearchKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape') {
    searchInput.value?.blur()
    searchFocused.value = false
  }
}

// Global "/" shortcut to focus search
function onGlobalKeydown(e: KeyboardEvent) {
  if (e.key === '/' && !['INPUT', 'TEXTAREA', 'SELECT'].includes((e.target as HTMLElement).tagName) && !(e.target as HTMLElement)?.isContentEditable) {
    e.preventDefault()
    searchInput.value?.focus()
  }
}

onMounted(() => {
  startPolling(15000)
  document.addEventListener('keydown', onGlobalKeydown)
  window.addEventListener('popstate', syncNavButtons)
  syncNavButtons()
})

onUnmounted(() => {
  document.removeEventListener('keydown', onGlobalKeydown)
  window.removeEventListener('popstate', syncNavButtons)
})

// --- Theme toggle dropdown ---
const themeMenuOpen = ref(false)
const themeMenuRef = ref<HTMLElement | null>(null)

function selectTheme(t: 'light' | 'dark' | 'system') {
  setTheme(t)
  themeMenuOpen.value = false
}

function onClickOutsideTheme(e: MouseEvent) {
  if (themeMenuRef.value && !themeMenuRef.value.contains(e.target as Node)) {
    themeMenuOpen.value = false
  }
}

onMounted(() => document.addEventListener('click', onClickOutsideTheme))
onUnmounted(() => document.removeEventListener('click', onClickOutsideTheme))

// --- User menu dropdown (Mark 2 style) ---
const userMenuOpen = ref(false)
const userMenuRef = ref<HTMLElement | null>(null)

function onClickOutsideUser(e: MouseEvent) {
  if (userMenuRef.value && !userMenuRef.value.contains(e.target as Node)) {
    userMenuOpen.value = false
  }
}

// Close dropdown on route change
import { watch } from 'vue'
watch(() => route.path, () => { userMenuOpen.value = false })
watch(() => route.fullPath, () => { syncNavButtons() })

onMounted(() => document.addEventListener('click', onClickOutsideUser))
onUnmounted(() => document.removeEventListener('click', onClickOutsideUser))

function navigateFromMenu(path: string) {
  userMenuOpen.value = false
  router.push(path)
}

async function handleLockAndSignOut() {
  userMenuOpen.value = false
  try { await lockVault() } catch (e) { console.warn('lock failed:', e) }
  router.replace('/unlock')
}

const userInitial = () => displayName.value ? displayName.value.charAt(0).toUpperCase() : 'A'
</script>

<template>
  <header :class="['topbar', isMac ? 'topbar--macos' : '']" data-tauri-drag-region>
    <!-- Left: Sidebar toggle -->
    <div class="topbar-left">
      <!-- Sidebar toggle (desktop only) -->
      <button
        v-if="!isMobilePlatform"
        class="topbar-icon-btn"
        :aria-label="sidebarCollapsed ? 'Expand sidebar' : 'Collapse sidebar'"
        :title="sidebarCollapsed ? 'Expand sidebar' : 'Collapse sidebar'"
        @click="emit('toggleSidebar')"
      >
        <svg class="h-[1.125rem] w-[1.125rem]" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75">
          <rect x="3" y="3" width="18" height="18" rx="3" stroke-linecap="round" stroke-linejoin="round" />
          <path d="M9 3v18" stroke-linecap="round" stroke-linejoin="round" />
        </svg>
      </button>

      <button
        class="topbar-icon-btn"
        :class="{ 'topbar-icon-btn--disabled': !canGoBack }"
        :disabled="!canGoBack"
        :aria-disabled="!canGoBack"
        aria-label="Go back"
        title="Back"
        @click="router.back()"
      >
        <svg class="h-[1.05rem] w-[1.05rem]" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9">
          <path stroke-linecap="round" stroke-linejoin="round" d="M15 19l-7-7 7-7" />
        </svg>
      </button>

      <button
        class="topbar-icon-btn"
        :class="{ 'topbar-icon-btn--disabled': !canGoForward }"
        :disabled="!canGoForward"
        :aria-disabled="!canGoForward"
        aria-label="Go forward"
        title="Forward"
        @click="router.forward()"
      >
        <svg class="h-[1.05rem] w-[1.05rem]" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9">
          <path stroke-linecap="round" stroke-linejoin="round" d="M9 5l7 7-7 7" />
        </svg>
      </button>
    </div>

    <!-- Center: Omni-search -->
    <div class="topbar-search-wrapper">
      <div :class="['topbar-search', searchFocused ? 'topbar-search--focused' : '']">
        <svg class="topbar-search-icon" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
          <path stroke-linecap="round" stroke-linejoin="round" d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z" />
        </svg>
        <input
          ref="searchInput"
          v-model="searchQuery"
          type="text"
          placeholder="Search courses, skills..."
          class="topbar-search-input"
          @focus="searchFocused = true"
          @blur="searchFocused = false"
          @keydown="onSearchKeydown"
        />
        <button
          v-if="searchQuery"
          type="button"
          class="topbar-search-clear"
          tabindex="-1"
          @mousedown.prevent="searchQuery = ''; searchInput?.focus()"
        >
          <svg class="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
            <path stroke-linecap="round" stroke-linejoin="round" d="M6 18L18 6M6 6l12 12" />
          </svg>
        </button>
        <kbd v-if="!searchQuery && !searchFocused" class="topbar-search-kbd">/</kbd>
      </div>
    </div>

    <!-- Right: P2P + theme + avatar -->
    <div class="topbar-right">
      <!-- P2P status — hidden on mobile -->
      <div class="hidden md:flex items-center gap-1.5 text-xs text-muted-foreground">
        <span
          class="w-2 h-2 rounded-full"
          :class="p2pStatus?.is_running
            ? 'bg-success'
            : p2pStatus != null ? 'bg-muted-foreground/40' : 'bg-warning animate-pulse'"
        />
        {{ p2pStatus?.is_running ? 'Connected' : p2pStatus != null ? 'Offline' : 'Starting...' }}
      </div>

      <!-- Theme toggle dropdown -->
      <div ref="themeMenuRef" class="theme-toggle relative">
        <button
          class="topbar-icon-btn"
          aria-haspopup="listbox"
          :aria-expanded="themeMenuOpen"
          :aria-label="`Current theme: ${theme}. Click to change.`"
          @click.stop="themeMenuOpen = !themeMenuOpen"
        >
          <svg class="w-[1.125rem] h-[1.125rem]" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
            <path v-if="theme === 'dark'" stroke-linecap="round" stroke-linejoin="round" d="M20.354 15.354A9 9 0 018.646 3.646 9.003 9.003 0 0012 21a9.003 9.003 0 008.354-5.646z" />
            <path v-else-if="theme === 'light'" stroke-linecap="round" stroke-linejoin="round" d="M12 3v1m0 16v1m9-9h-1M4 12H3m15.364 6.364l-.707-.707M6.343 6.343l-.707-.707m12.728 0l-.707.707M6.343 17.657l-.707.707M16 12a4 4 0 11-8 0 4 4 0 018 0z" />
            <path v-else stroke-linecap="round" stroke-linejoin="round" d="M9.75 17L9 20l-1 1h8l-1-1-.75-3M3 13h18M5 17h14a2 2 0 002-2V5a2 2 0 00-2-2H5a2 2 0 00-2 2v10a2 2 0 002 2z" />
          </svg>
        </button>

        <Transition
          enter-active-class="transition duration-100 ease-out"
          enter-from-class="transform scale-95 opacity-0"
          enter-to-class="transform scale-100 opacity-100"
          leave-active-class="transition duration-75 ease-in"
          leave-from-class="transform scale-100 opacity-100"
          leave-to-class="transform scale-95 opacity-0"
        >
          <div v-if="themeMenuOpen" class="absolute right-0 mt-2 w-36 origin-top-right rounded-lg border border-border bg-card shadow-lg z-50">
            <div class="p-1">
              <button
                v-for="opt in [
                  { value: 'light' as const, label: 'Light', icon: 'M12 3v1m0 16v1m9-9h-1M4 12H3m15.364 6.364l-.707-.707M6.343 6.343l-.707-.707m12.728 0l-.707.707M6.343 17.657l-.707.707M16 12a4 4 0 11-8 0 4 4 0 018 0z' },
                  { value: 'dark' as const, label: 'Dark', icon: 'M20.354 15.354A9 9 0 018.646 3.646 9.003 9.003 0 0012 21a9.003 9.003 0 008.354-5.646z' },
                  { value: 'system' as const, label: 'System', icon: 'M9.75 17L9 20l-1 1h8l-1-1-.75-3M3 13h18M5 17h14a2 2 0 002-2V5a2 2 0 00-2-2H5a2 2 0 00-2 2v10a2 2 0 002 2z' },
                ]"
                :key="opt.value"
                class="flex items-center gap-2 w-full px-3 py-2 text-sm rounded-md transition-colors text-left"
                :class="theme === opt.value
                  ? 'bg-primary/10 text-primary font-medium'
                  : 'text-foreground hover:bg-muted'"
                role="option"
                :aria-selected="theme === opt.value"
                @click="selectTheme(opt.value)"
              >
                <svg class="w-4 h-4 shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                  <path stroke-linecap="round" stroke-linejoin="round" :d="opt.icon" />
                </svg>
                {{ opt.label }}
              </button>
            </div>
          </div>
        </Transition>
      </div>

      <!-- User avatar with dropdown -->
      <div ref="userMenuRef" class="user-menu relative">
        <button
          class="flex h-8 w-8 items-center justify-center rounded-full bg-gradient-to-br from-primary to-accent text-xs font-bold text-white transition-shadow hover:shadow-md"
          :aria-label="`User menu for ${displayName || 'User'}`"
          aria-haspopup="true"
          :aria-expanded="userMenuOpen"
          @click.stop="userMenuOpen = !userMenuOpen"
        >
          {{ userInitial() }}
        </button>

        <Transition
          enter-active-class="transition duration-100 ease-out"
          enter-from-class="transform scale-95 opacity-0"
          enter-to-class="transform scale-100 opacity-100"
          leave-active-class="transition duration-75 ease-in"
          leave-from-class="transform scale-100 opacity-100"
          leave-to-class="transform scale-95 opacity-0"
        >
          <div v-if="userMenuOpen" class="absolute right-0 mt-2 w-56 origin-top-right rounded-xl border border-border bg-card shadow-lg z-50">
            <!-- User info header -->
            <div class="border-b border-border px-4 py-3">
              <div class="flex items-center gap-2">
                <p class="truncate text-sm font-medium text-foreground">
                  {{ displayName || 'Anonymous' }}
                </p>
                <span class="inline-block rounded-full bg-muted px-1.5 py-0.5 text-[10px] font-medium capitalize leading-none text-muted-foreground">
                  learner
                </span>
              </div>
              <p class="truncate text-xs text-muted-foreground mt-0.5">
                Local vault user
              </p>
            </div>

            <!-- Navigation links -->
            <div class="p-1">
              <!-- My Courses -->
              <button class="flex items-center gap-2 rounded-lg px-3 py-2 w-full text-sm text-foreground transition-colors hover:bg-muted" @click="navigateFromMenu('/dashboard/courses')">
                <svg class="h-4 w-4 text-muted-foreground" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M12 6.253v13m0-13C10.832 5.477 9.246 5 7.5 5S4.168 5.477 3 6.253v13C4.168 18.477 5.754 18 7.5 18s3.332.477 4.5 1.253m0-13C13.168 5.477 14.754 5 16.5 5c1.747 0 3.332.477 4.5 1.253v13C19.832 18.477 18.247 18 16.5 18c-1.746 0-3.332.477-4.5 1.253" />
                </svg>
                My Courses
              </button>

              <!-- My Credentials -->
              <button class="flex items-center gap-2 rounded-lg px-3 py-2 w-full text-sm text-foreground transition-colors hover:bg-muted" @click="navigateFromMenu('/dashboard/credentials')">
                <svg class="h-4 w-4 text-muted-foreground" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M9 12l2 2 4-4m5.618-4.016A11.955 11.955 0 0112 2.944a11.955 11.955 0 01-8.618 3.04A12.02 12.02 0 003 9c0 5.591 3.824 10.29 9 11.622 5.176-1.332 9-6.03 9-11.622 0-1.042-.133-2.052-.382-3.016z" />
                </svg>
                My Credentials
              </button>

              <!-- My Skills -->
              <button class="flex items-center gap-2 rounded-lg px-3 py-2 w-full text-sm text-foreground transition-colors hover:bg-muted" @click="navigateFromMenu('/skills')">
                <svg class="h-4 w-4 text-muted-foreground" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M13 10V3L4 14h7v7l9-11h-7z" />
                </svg>
                My Skills
              </button>

              <!-- My Reputation -->
              <button class="flex items-center gap-2 rounded-lg px-3 py-2 w-full text-sm text-foreground transition-colors hover:bg-muted" @click="navigateFromMenu('/dashboard/reputation')">
                <svg class="h-4 w-4 text-muted-foreground" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M11.049 2.927c.3-.921 1.603-.921 1.902 0l1.519 4.674a1 1 0 00.95.69h4.915c.969 0 1.371 1.24.588 1.81l-3.976 2.888a1 1 0 00-.363 1.118l1.518 4.674c.3.922-.755 1.688-1.538 1.118l-3.976-2.888a1 1 0 00-1.176 0l-3.976 2.888c-.783.57-1.838-.197-1.538-1.118l1.518-4.674a1 1 0 00-.363-1.118l-3.976-2.888c-.784-.57-.38-1.81.588-1.81h4.914a1 1 0 00.951-.69l1.519-4.674z" />
                </svg>
                My Reputation
              </button>

              <!-- Sentinel -->
              <button class="flex items-center gap-2 rounded-lg px-3 py-2 w-full text-sm text-foreground transition-colors hover:bg-muted" @click="navigateFromMenu('/dashboard/sentinel')">
                <svg class="h-4 w-4 text-muted-foreground" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M9 12.75L11.25 15 15 9.75m-3-7.036A11.959 11.959 0 013.598 6 11.99 11.99 0 003 9.749c0 5.592 3.824 10.29 9 11.623 5.176-1.332 9-6.03 9-11.622 0-1.31-.21-2.571-.598-3.751h-.152c-3.196 0-6.1-1.248-8.25-3.285z" />
                </svg>
                Sentinel
              </button>

              <!-- Governance -->
              <button class="flex items-center gap-2 rounded-lg px-3 py-2 w-full text-sm text-foreground transition-colors hover:bg-muted" @click="navigateFromMenu('/governance')">
                <svg class="h-4 w-4 text-muted-foreground" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M3 21h18M3 10h18M5 6l7-3 7 3M4 10v11m16-11v11M8 14v3m4-3v3m4-3v3" />
                </svg>
                Governance
              </button>

              <!-- Settings -->
              <button class="flex items-center gap-2 rounded-lg px-3 py-2 w-full text-sm text-foreground transition-colors hover:bg-muted" @click="navigateFromMenu('/dashboard/settings')">
                <svg class="h-4 w-4 text-muted-foreground" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.066 2.573c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.573 1.066c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.066-2.573c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z" />
                  <path stroke-linecap="round" stroke-linejoin="round" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" />
                </svg>
                Settings
              </button>
            </div>

            <div class="border-t border-border p-1">
              <button
                class="flex w-full items-center gap-2 rounded-lg px-3 py-2 text-sm text-error transition-colors hover:bg-error/10"
                @click="handleLockAndSignOut"
              >
                <svg class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M17 16l4-4m0 0l-4-4m4 4H7m6 4v1a3 3 0 01-3 3H6a3 3 0 01-3-3V7a3 3 0 013-3h4a3 3 0 013 3v1" />
                </svg>
                Lock &amp; Sign Out
              </button>
            </div>

          </div>
        </Transition>
      </div>
    </div>
  </header>
</template>

<style scoped>
/* =========================================
   Topbar Layout
   ========================================= */

.topbar {
  display: flex;
  align-items: center;
  height: 3rem;
  padding: 0 0.75rem;
  border-bottom: 1px solid var(--app-border);
  background: var(--app-background);
  gap: 0.5rem;
  flex-shrink: 0;
}

.topbar--macos {
  padding-left: 5rem;
}

.topbar-left {
  display: flex;
  align-items: center;
  gap: 0;
  flex-shrink: 0;
}

.topbar-right {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  flex-shrink: 0;
}

/* Icon buttons (sidebar toggle, app icon) */
.topbar-icon-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 2.25rem;
  height: 2rem;
  border-radius: 0.5rem;
  color: var(--app-muted-foreground);
  background: transparent;
  border: none;
  cursor: pointer;
  transition: color 0.15s, background 0.15s;
}

.topbar button,
.topbar input,
.topbar [role='option'],
.topbar .user-menu,
.topbar .theme-toggle {
  -webkit-app-region: no-drag;
  app-region: no-drag;
}

.topbar-icon-btn:hover {
  color: var(--app-foreground);
  background: color-mix(in srgb, var(--app-muted) 50%, transparent);
}

.topbar-icon-btn--disabled,
.topbar-icon-btn:disabled {
  opacity: 0.42;
  cursor: default;
  pointer-events: none;
}

/* =========================================
   Topbar Omni-search
   ========================================= */

.topbar-search-wrapper {
  flex: 1 1 0%;
  display: flex;
  justify-content: center;
  min-width: 0;
  padding: 0 0.25rem;
}

.topbar-search {
  position: relative;
  display: flex;
  align-items: center;
  width: 100%;
  max-width: 32rem;
  background: color-mix(in srgb, var(--app-muted) 35%, transparent);
  border: 1px solid transparent;
  border-radius: 0.5rem;
  transition:
    background 0.2s,
    border-color 0.2s,
    box-shadow 0.2s;
}

.topbar-search:hover {
  background: color-mix(in srgb, var(--app-muted) 55%, transparent);
}

.topbar-search--focused {
  background: var(--app-card);
  border-color: color-mix(in srgb, var(--app-primary) 40%, transparent);
  box-shadow: 0 0 0 3px color-mix(in srgb, var(--app-primary) 8%, transparent);
}

.topbar-search--focused:hover {
  background: var(--app-card);
}

.topbar-search-icon {
  position: absolute;
  left: 0.625rem;
  width: 0.875rem;
  height: 0.875rem;
  color: color-mix(in srgb, var(--app-muted-foreground) 50%, transparent);
  pointer-events: none;
  flex-shrink: 0;
}

.topbar-search-input {
  width: 100%;
  padding: 0.375rem 2rem 0.375rem 2rem;
  font-size: 1rem;       /* 16px on mobile — prevents iOS Safari auto-zoom on focus */
  line-height: 1.5;
  color: var(--app-foreground);
  background: transparent;
  border: none;
  outline: none;
}

@media (min-width: 640px) {
  .topbar-search-input {
    font-size: 0.8125rem; /* Restore compact size on desktop */
  }
}

.topbar-search-input::placeholder {
  color: color-mix(in srgb, var(--app-muted-foreground) 50%, transparent);
}

.topbar-search-clear {
  position: absolute;
  right: 0.375rem;
  display: flex;
  align-items: center;
  justify-content: center;
  width: 1.25rem;
  height: 1.25rem;
  border-radius: 0.25rem;
  color: color-mix(in srgb, var(--app-muted-foreground) 60%, transparent);
  transition: color 0.15s, background 0.15s;
  cursor: pointer;
  background: transparent;
  border: none;
}

.topbar-search-clear:hover {
  color: var(--app-foreground);
  background: color-mix(in srgb, var(--app-muted) 50%, transparent);
}

.topbar-search-kbd {
  position: absolute;
  right: 0.5rem;
  display: flex;
  align-items: center;
  justify-content: center;
  width: 1.25rem;
  height: 1.25rem;
  font-size: 0.625rem;
  font-weight: 500;
  color: color-mix(in srgb, var(--app-muted-foreground) 45%, transparent);
  background: color-mix(in srgb, var(--app-muted) 50%, transparent);
  border: 1px solid color-mix(in srgb, var(--app-border) 40%, transparent);
  border-radius: 0.25rem;
  pointer-events: none;
}

/* Dark mode search overrides */
:is(.dark *) .topbar-search {
  background: color-mix(in srgb, white 5%, transparent);
}

:is(.dark *) .topbar-search:hover {
  background: color-mix(in srgb, white 8%, transparent);
}

:is(.dark *) .topbar-search--focused {
  background: var(--app-card);
  box-shadow: 0 0 0 3px color-mix(in srgb, var(--app-primary) 15%, transparent);
}

:is(.dark *) .topbar-search--focused:hover {
  background: var(--app-card);
}
</style>
