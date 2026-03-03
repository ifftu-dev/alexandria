<script setup lang="ts">
import { ref, onMounted, onUnmounted } from 'vue'
import { useRouter, useRoute } from 'vue-router'
import { useP2P } from '@/composables/useP2P'
import { useAuth } from '@/composables/useAuth'
import SidebarSkillGraph from '@/components/layout/SidebarSkillGraph.vue'

defineProps<{ collapsed: boolean }>()
const emit = defineEmits<{ toggle: [] }>()
const router = useRouter()
const route = useRoute()
const { status: p2pStatus, startPolling } = useP2P()
const { lockVault } = useAuth()

// Keyboard shortcut: Cmd+\ (macOS) / Ctrl+\ (Windows)
function onKeyDown(e: KeyboardEvent) {
  if ((e.metaKey || e.ctrlKey) && e.key === '\\') {
    e.preventDefault()
    emit('toggle')
  }
}

onMounted(() => {
  startPolling(15000)
  document.addEventListener('keydown', onKeyDown)
})

onUnmounted(() => {
  document.removeEventListener('keydown', onKeyDown)
})

async function signOut() {
  try { await lockVault() } catch (e) { console.warn('lock failed:', e) }
  router.replace('/unlock')
}

const isActive = (path: string) => {
  if (path === '/home') return route.path === '/home'
  return route.path.startsWith(path)
}

function navigate(path: string) {
  router.push(path)
}

// =========================================
// Collapsible section state (persisted)
// =========================================
type SectionKey = 'tutoring' | 'classrooms'

const sectionState = ref<Record<string, boolean>>({ tutoring: true, classrooms: true })

onMounted(() => {
  try {
    const stored = localStorage.getItem('sidebar-sections')
    if (stored) sectionState.value = JSON.parse(stored)
  } catch { /* ignore */ }
})

function toggleSection(key: SectionKey) {
  sectionState.value = { ...sectionState.value, [key]: !sectionState.value[key] }
  localStorage.setItem('sidebar-sections', JSON.stringify(sectionState.value))
}

const isSectionOpen = (key: SectionKey) => sectionState.value[key] !== false

// =========================================
// Mock data: Live Tutoring sessions
// =========================================
const tutoringPreviews = [
  { id: '1', title: 'Graph Algorithms Deep Dive', tutor_name: 'Prof. Sarah Chen', tutor_initials: 'SC', status: 'live' as const },
  { id: '2', title: 'Intro to Smart Contracts', tutor_name: 'Dr. Marcus Webb', tutor_initials: 'MW', status: 'starting-soon' as const },
  { id: '3', title: 'Database Optimization Patterns', tutor_name: 'Prof. Lena Okafor', tutor_initials: 'LO', status: 'scheduled' as const },
  { id: '4', title: 'Functional Programming in Haskell', tutor_name: 'Dr. Raj Patel', tutor_initials: 'RP', status: 'ended' as const },
]

// =========================================
// Mock data: Classrooms
// =========================================
const classroomPreviews = [
  { id: '1', name: 'Advanced Algorithms', initial: 'A', member_count: 24, active: true, unread_count: 3 },
  { id: '2', name: 'Cardano Dev Cohort #7', initial: 'C', member_count: 18, active: true, unread_count: 0 },
  { id: '3', name: 'ML Study Group', initial: 'M', member_count: 31, active: false, unread_count: 12 },
  { id: '4', name: 'Web3 Builders', initial: 'W', member_count: 9, active: false, unread_count: 0 },
]
</script>

<template>
  <aside
    :class="[
      'relative flex h-full flex-col overflow-hidden border-r border-border bg-background transition-all duration-300',
      collapsed ? 'w-16' : 'w-64 md:overflow-visible',
    ]"
  >
    <!-- Logo -->
    <div class="flex h-14 items-center border-b border-border px-4 shrink-0">
      <button class="flex items-center gap-2 text-foreground transition-opacity hover:opacity-80" @click="navigate('/home')">
        <svg class="h-7 w-7 shrink-0 text-primary" viewBox="0 0 32 32" fill="none">
          <path d="M16 2L4 8v16l12 6 12-6V8L16 2z" stroke="currentColor" stroke-width="2" fill="none" />
          <path d="M16 8v16M8 12l8 4 8-4" stroke="currentColor" stroke-width="2" />
        </svg>
        <span :class="['text-lg font-semibold transition-opacity duration-300 whitespace-nowrap', collapsed ? 'opacity-0 w-0 overflow-hidden' : 'opacity-100']">Alexandria</span>
      </button>
    </div>

    <!-- Edge toggle -->
    <button type="button" class="sb-edge-toggle" :title="collapsed ? 'Expand sidebar' : 'Collapse sidebar'" @click="emit('toggle')">
      <svg :class="['h-3 w-3 transition-transform duration-200', collapsed ? 'rotate-180' : '']" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2.5">
        <path stroke-linecap="round" stroke-linejoin="round" d="M15 19l-7-7 7-7" />
      </svg>
    </button>

    <!-- Scrollable nav area -->
    <nav class="flex-1 overflow-y-auto overflow-x-hidden px-2 py-3">

      <!-- ========================================= -->
      <!-- Section 1: Home -->
      <!-- ========================================= -->
      <button
        :class="['group relative flex items-center gap-3 w-full rounded-lg px-3 py-2.5 text-sm font-medium transition-colors', isActive('/home') ? 'text-primary' : 'text-muted-foreground hover:bg-muted hover:text-foreground']"
        @click="navigate('/home')"
      >
        <div v-if="isActive('/home')" class="absolute left-0 top-1 bottom-1 w-0.5 rounded-full bg-primary" />
        <svg class="h-5 w-5 shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
          <path stroke-linecap="round" stroke-linejoin="round" d="M3 12l2-2m0 0l7-7 7 7M5 10v10a1 1 0 001 1h3m10-11l2 2m-2-2v10a1 1 0 01-1 1h-3m-6 0a1 1 0 001-1v-4a1 1 0 011-1h2a1 1 0 011 1v4a1 1 0 001 1m-6 0h6" />
        </svg>
        <span :class="['transition-opacity duration-300 whitespace-nowrap', collapsed ? 'opacity-0 w-0 overflow-hidden' : 'opacity-100']">Home</span>
        <div v-if="collapsed" class="sb-collapsed-tooltip">Home</div>
      </button>

      <!-- ========================================= -->
      <!-- Section 2: Live Tutoring (collapsible inline previews) -->
      <!-- ========================================= -->
      <div class="mt-4">
        <div :class="['mx-1 mb-1', collapsed ? 'flex justify-center' : '']">
          <!-- Collapsed: just the icon -->
          <button
            v-if="collapsed"
            :class="['group relative flex items-center justify-center rounded-lg px-3 py-2.5 transition-colors', isActive('/tutoring') ? 'text-primary' : 'text-muted-foreground hover:bg-muted hover:text-foreground']"
            @click="navigate('/courses')"
          >
            <svg class="h-5 w-5 shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
              <path stroke-linecap="round" stroke-linejoin="round" d="M15 10l4.553-2.276A1 1 0 0121 8.618v6.764a1 1 0 01-1.447.894L15 14M5 18h8a2 2 0 002-2V8a2 2 0 00-2-2H5a2 2 0 00-2 2v8a2 2 0 002 2z" />
            </svg>
            <div class="sb-collapsed-tooltip">Live Tutoring</div>
          </button>

          <!-- Expanded: header with chevron -->
          <div :class="['flex items-center gap-1', collapsed ? 'hidden' : '']">
            <button
              :class="['flex flex-1 items-center gap-2 rounded-lg px-3 py-2 text-[0.8125rem] font-semibold uppercase tracking-wider transition-colors', isActive('/tutoring') ? 'text-primary' : 'text-muted-foreground hover:text-foreground']"
              @click="navigate('/courses')"
            >
              <svg class="h-4 w-4 shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                <path stroke-linecap="round" stroke-linejoin="round" d="M15 10l4.553-2.276A1 1 0 0121 8.618v6.764a1 1 0 01-1.447.894L15 14M5 18h8a2 2 0 002-2V8a2 2 0 00-2-2H5a2 2 0 00-2 2v8a2 2 0 002 2z" />
              </svg>
              Live Tutoring
            </button>
            <button
              type="button"
              class="rounded p-1 text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
              @click.stop="toggleSection('tutoring')"
            >
              <svg :class="['h-3.5 w-3.5 transition-transform duration-200', isSectionOpen('tutoring') ? 'rotate-0' : '-rotate-90']" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2.5">
                <path stroke-linecap="round" stroke-linejoin="round" d="M19 9l-7 7-7-7" />
              </svg>
            </button>
          </div>
        </div>

        <!-- Tutoring inline previews -->
        <Transition
          enter-active-class="transition-all duration-200 ease-out"
          enter-from-class="max-h-0 opacity-0"
          enter-to-class="max-h-96 opacity-100"
          leave-active-class="transition-all duration-150 ease-in"
          leave-from-class="max-h-96 opacity-100"
          leave-to-class="max-h-0 opacity-0"
        >
          <div v-if="isSectionOpen('tutoring') && !collapsed" class="sb-preview-list overflow-hidden">
            <button
              v-for="session in tutoringPreviews"
              :key="session.id"
              class="sb-preview-card group"
              @click="navigate('/courses')"
            >
              <div class="sb-avatar">{{ session.tutor_initials }}</div>
              <div class="min-w-0 flex-1">
                <div class="flex items-center gap-1.5">
                  <span class="sb-preview-title">{{ session.tutor_name }}</span>
                </div>
                <span class="sb-preview-meta sb-marquee-wrap">
                  <span class="sb-marquee-text">{{ session.title }}</span>
                </span>
              </div>
              <!-- Live: pulsing red dot -->
              <span v-if="session.status === 'live'" class="sb-status-icon" title="Live now"><span class="sb-live-dot" /></span>
              <!-- Starting soon: amber dot -->
              <span v-else-if="session.status === 'starting-soon'" class="sb-status-icon" title="Starting soon"><span class="sb-starting-dot" /></span>
              <!-- Scheduled: clock icon -->
              <span v-else-if="session.status === 'scheduled'" class="sb-status-icon sb-status-scheduled" title="Scheduled">
                <svg class="h-3 w-3" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2.5"><path stroke-linecap="round" stroke-linejoin="round" d="M12 6v6l4 2m6-2a10 10 0 11-20 0 10 10 0 0120 0z" /></svg>
              </span>
              <!-- Ended: checkmark -->
              <span v-else-if="session.status === 'ended'" class="sb-status-icon sb-status-ended" title="Ended">
                <svg class="h-3 w-3" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2.5"><path stroke-linecap="round" stroke-linejoin="round" d="M5 13l4 4L19 7" /></svg>
              </span>
            </button>

            <button class="sb-view-all" @click="navigate('/courses')">
              View all sessions
              <svg class="h-3 w-3" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2.5"><path stroke-linecap="round" stroke-linejoin="round" d="M9 5l7 7-7 7" /></svg>
            </button>
          </div>
        </Transition>
      </div>

      <!-- ========================================= -->
      <!-- Section 3: Classrooms (collapsible inline previews) -->
      <!-- ========================================= -->
      <div class="mt-4">
        <div :class="['mx-1 mb-1', collapsed ? 'flex justify-center' : '']">
          <!-- Collapsed: just the icon -->
          <button
            v-if="collapsed"
            :class="['group relative flex items-center justify-center rounded-lg px-3 py-2.5 transition-colors', isActive('/classrooms') ? 'text-primary' : 'text-muted-foreground hover:bg-muted hover:text-foreground']"
            @click="navigate('/courses')"
          >
            <svg class="h-5 w-5 shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
              <path stroke-linecap="round" stroke-linejoin="round" d="M19 21V5a2 2 0 00-2-2H7a2 2 0 00-2 2v16m14 0h2m-2 0h-5m-9 0H3m2 0h5M9 7h1m-1 4h1m4-4h1m-1 4h1m-5 10v-5a1 1 0 011-1h2a1 1 0 011 1v5m-4 0h4" />
            </svg>
            <div class="sb-collapsed-tooltip">Classrooms</div>
          </button>

          <!-- Expanded: header with chevron -->
          <div :class="['flex items-center gap-1', collapsed ? 'hidden' : '']">
            <button
              :class="['flex flex-1 items-center gap-2 rounded-lg px-3 py-2 text-[0.8125rem] font-semibold uppercase tracking-wider transition-colors', isActive('/classrooms') ? 'text-primary' : 'text-muted-foreground hover:text-foreground']"
              @click="navigate('/courses')"
            >
              <svg class="h-4 w-4 shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                <path stroke-linecap="round" stroke-linejoin="round" d="M19 21V5a2 2 0 00-2-2H7a2 2 0 00-2 2v16m14 0h2m-2 0h-5m-9 0H3m2 0h5M9 7h1m-1 4h1m4-4h1m-1 4h1m-5 10v-5a1 1 0 011-1h2a1 1 0 011 1v5m-4 0h4" />
              </svg>
              Classrooms
            </button>
            <button
              type="button"
              class="rounded p-1 text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
              @click.stop="toggleSection('classrooms')"
            >
              <svg :class="['h-3.5 w-3.5 transition-transform duration-200', isSectionOpen('classrooms') ? 'rotate-0' : '-rotate-90']" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2.5">
                <path stroke-linecap="round" stroke-linejoin="round" d="M19 9l-7 7-7-7" />
              </svg>
            </button>
          </div>
        </div>

        <!-- Classroom inline previews -->
        <Transition
          enter-active-class="transition-all duration-200 ease-out"
          enter-from-class="max-h-0 opacity-0"
          enter-to-class="max-h-96 opacity-100"
          leave-active-class="transition-all duration-150 ease-in"
          leave-from-class="max-h-96 opacity-100"
          leave-to-class="max-h-0 opacity-0"
        >
          <div v-if="isSectionOpen('classrooms') && !collapsed" class="sb-preview-list overflow-hidden">
            <button
              v-for="classroom in classroomPreviews"
              :key="classroom.id"
              class="sb-preview-card group"
              @click="navigate('/courses')"
            >
              <div class="sb-avatar sb-avatar--classroom">{{ classroom.initial }}</div>
              <div class="min-w-0 flex-1">
                <div class="flex items-center gap-1.5">
                  <span class="sb-preview-title">{{ classroom.name }}</span>
                  <span v-if="classroom.unread_count > 0" class="sb-unread-badge">{{ classroom.unread_count }}</span>
                </div>
                <span class="sb-preview-meta">{{ classroom.member_count }} members</span>
              </div>
              <span v-if="classroom.active" class="sb-status-icon" title="Active"><span class="sb-active-dot" /></span>
            </button>

            <button class="sb-view-all" @click="navigate('/courses')">
              View all classrooms
              <svg class="h-3 w-3" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2.5"><path stroke-linecap="round" stroke-linejoin="round" d="M9 5l7 7-7 7" /></svg>
            </button>
          </div>
        </Transition>
      </div>

    </nav>

    <!-- ========================================= -->
    <!-- Skill Graph (above bottom nav, hidden when collapsed) -->
    <!-- ========================================= -->
    <div
      :class="[
        'overflow-hidden transition-all duration-300 border-t border-border px-2 py-2',
        collapsed ? 'max-h-0 opacity-0 !py-0 !border-0' : 'max-h-64 opacity-100',
      ]"
    >
      <SidebarSkillGraph />
    </div>

    <!-- ========================================= -->
    <!-- Fixed bottom — Governance, Sentinel, Network, Settings, Lock & Sign Out -->
    <!-- ========================================= -->
    <div class="border-t border-border px-2 py-2 shrink-0">
      <!-- P2P status row (expanded only) -->
      <div v-if="!collapsed" class="flex items-center gap-2 px-3 py-1.5 mb-1 text-xs text-muted-foreground">
        <span class="w-2 h-2 rounded-full shrink-0" :class="p2pStatus?.is_running ? 'bg-success' : p2pStatus != null ? 'bg-muted-foreground/40' : 'bg-amber-500 animate-pulse'" />
        {{ p2pStatus?.is_running ? `${p2pStatus.connected_peers} peer${p2pStatus.connected_peers !== 1 ? 's' : ''} connected` : p2pStatus != null ? 'Offline' : 'Starting...' }}
      </div>

      <!-- Governance -->
      <button :class="['group relative flex items-center gap-3 w-full rounded-lg px-3 py-2 text-sm font-medium transition-colors', isActive('/governance') ? 'text-primary' : 'text-muted-foreground hover:bg-muted hover:text-foreground']" @click="navigate('/governance')">
        <div v-if="isActive('/governance')" class="absolute left-0 top-1 bottom-1 w-0.5 rounded-full bg-primary" />
        <svg class="h-5 w-5 shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2"><path stroke-linecap="round" stroke-linejoin="round" d="M3 21h18M3 10h18M5 6l7-3 7 3M4 10v11m16-11v11M8 14v3m4-3v3m4-3v3" /></svg>
        <span :class="['transition-opacity duration-300 whitespace-nowrap', collapsed ? 'opacity-0 w-0 overflow-hidden' : 'opacity-100']">Community Proposals</span>
        <div v-if="collapsed" class="sb-collapsed-tooltip">Governance</div>
      </button>

      <!-- Sentinel -->
      <button :class="['group relative flex items-center gap-3 w-full rounded-lg px-3 py-2 text-sm font-medium transition-colors', isActive('/dashboard/sentinel') ? 'text-primary' : 'text-muted-foreground hover:bg-muted hover:text-foreground']" @click="navigate('/dashboard/sentinel')">
        <div v-if="isActive('/dashboard/sentinel')" class="absolute left-0 top-1 bottom-1 w-0.5 rounded-full bg-primary" />
        <svg class="h-5 w-5 shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2"><path stroke-linecap="round" stroke-linejoin="round" d="M9 12.75L11.25 15 15 9.75m-3-7.036A11.959 11.959 0 013.598 6 11.99 11.99 0 003 9.749c0 5.592 3.824 10.29 9 11.623 5.176-1.332 9-6.03 9-11.622 0-1.31-.21-2.571-.598-3.751h-.152c-3.196 0-6.1-1.248-8.25-3.285z" /></svg>
        <span :class="['transition-opacity duration-300 whitespace-nowrap', collapsed ? 'opacity-0 w-0 overflow-hidden' : 'opacity-100']">Sentinel</span>
        <div v-if="collapsed" class="sb-collapsed-tooltip">Sentinel</div>
      </button>

      <!-- Network -->
      <button :class="['group relative flex items-center gap-3 w-full rounded-lg px-3 py-2 text-sm font-medium transition-colors', isActive('/dashboard/network') ? 'text-primary' : 'text-muted-foreground hover:bg-muted hover:text-foreground']" @click="navigate('/dashboard/network')">
        <div v-if="isActive('/dashboard/network')" class="absolute left-0 top-1 bottom-1 w-0.5 rounded-full bg-primary" />
        <svg class="h-5 w-5 shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2"><path stroke-linecap="round" stroke-linejoin="round" d="M21 12a9 9 0 01-9 9m9-9a9 9 0 00-9-9m9 9H3m9 9a9 9 0 01-9-9m9 9c1.657 0 3-4.03 3-9s-1.343-9-3-9m0 18c-1.657 0-3-4.03-3-9s1.343-9 3-9m-9 9a9 9 0 019-9" /></svg>
        <span :class="['transition-opacity duration-300 whitespace-nowrap', collapsed ? 'opacity-0 w-0 overflow-hidden' : 'opacity-100']">Network</span>
        <div v-if="collapsed" class="sb-collapsed-tooltip">Network</div>
      </button>

      <!-- Settings -->
      <button :class="['group relative flex items-center gap-3 w-full rounded-lg px-3 py-2 text-sm font-medium transition-colors', isActive('/dashboard/settings') ? 'text-primary' : 'text-muted-foreground hover:bg-muted hover:text-foreground']" @click="navigate('/dashboard/settings')">
        <div v-if="isActive('/dashboard/settings')" class="absolute left-0 top-1 bottom-1 w-0.5 rounded-full bg-primary" />
        <svg class="h-5 w-5 shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2"><path stroke-linecap="round" stroke-linejoin="round" d="M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.066 2.573c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.573 1.066c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.066-2.573c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z" /><path stroke-linecap="round" stroke-linejoin="round" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" /></svg>
        <span :class="['transition-opacity duration-300 whitespace-nowrap', collapsed ? 'opacity-0 w-0 overflow-hidden' : 'opacity-100']">Settings</span>
        <div v-if="collapsed" class="sb-collapsed-tooltip">Settings</div>
      </button>

      <!-- Lock & Sign Out -->
      <button class="group relative flex items-center gap-3 w-full rounded-lg px-3 py-2 text-sm font-medium text-error/80 hover:bg-error/10 hover:text-error transition-colors" @click="signOut">
        <svg class="h-5 w-5 shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2"><path stroke-linecap="round" stroke-linejoin="round" d="M15.75 9V5.25A2.25 2.25 0 0013.5 3h-6a2.25 2.25 0 00-2.25 2.25v13.5A2.25 2.25 0 007.5 21h6a2.25 2.25 0 002.25-2.25V15m3 0l3-3m0 0l-3-3m3 3H9" /></svg>
        <span :class="['transition-opacity duration-300 whitespace-nowrap', collapsed ? 'opacity-0 w-0 overflow-hidden' : 'opacity-100']">Lock &amp; Sign Out</span>
        <div v-if="collapsed" class="sb-collapsed-tooltip">Lock</div>
      </button>
    </div>
  </aside>
</template>

<style scoped>
/* =========================================
   Edge Toggle
   ========================================= */

.sb-edge-toggle {
  display: none;
  position: absolute;
  right: -0.6875rem;
  top: 1.75rem;
  transform: translateY(-50%);
  z-index: 60;
  width: 1.375rem;
  height: 1.375rem;
  align-items: center;
  justify-content: center;
  border-radius: 50%;
  background: color-mix(in srgb, var(--app-primary) 6%, transparent);
  border: 1px solid color-mix(in srgb, var(--app-primary) 30%, transparent);
  color: var(--app-primary);
  cursor: pointer;
  transition: color 0.2s ease, background 0.2s ease, border-color 0.2s ease, box-shadow 0.2s ease;
  box-shadow: 0 1px 3px rgb(0 0 0 / 0.06);
}

@media (min-width: 768px) {
  .sb-edge-toggle { display: flex; }
}

.sb-edge-toggle:hover {
  border-color: color-mix(in srgb, var(--app-primary) 50%, transparent);
  background: var(--app-card);
  box-shadow: 0 1px 3px rgb(0 0 0 / 0.06), 0 0 0 2px color-mix(in srgb, var(--app-primary) 10%, transparent);
}

.sb-edge-toggle:active { transform: translateY(-50%) scale(0.88); }

:is(.dark *) .sb-edge-toggle {
  background: rgb(24 24 28);
  border-color: rgb(255 255 255 / 0.1);
  box-shadow: 0 1px 4px rgb(0 0 0 / 0.4);
}

:is(.dark *) .sb-edge-toggle:hover {
  background: rgb(32 32 38);
  border-color: color-mix(in srgb, var(--app-primary) 50%, transparent);
  box-shadow: 0 1px 4px rgb(0 0 0 / 0.4), 0 0 0 2px color-mix(in srgb, var(--app-primary) 15%, transparent);
}

/* =========================================
   Collapsed Tooltip
   ========================================= */

.sb-collapsed-tooltip {
  position: absolute;
  left: 100%;
  top: 50%;
  transform: translateY(-50%);
  margin-left: 0.5rem;
  padding: 0.25rem 0.5rem;
  font-size: 0.75rem;
  font-weight: 500;
  white-space: nowrap;
  color: var(--app-background);
  background: var(--app-foreground);
  border-radius: 0.375rem;
  box-shadow: var(--shadow-lg);
  z-index: 70;
  pointer-events: none;
  opacity: 0;
  transition: opacity 0.15s;
}

.group:hover .sb-collapsed-tooltip { opacity: 1; }

/* =========================================
   Sidebar Inline Preview Cards
   (Live Tutoring & Classrooms)
   ========================================= */

.sb-preview-list {
  padding: 0.25rem 0.5rem 0.5rem;
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
}

.sb-preview-card {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  padding: 0.4375rem 0.5rem;
  border-radius: 0.5rem;
  transition: background 0.15s;
  text-decoration: none;
  cursor: pointer;
  text-align: left;
  width: 100%;
  background: transparent;
  border: none;
  color: inherit;
}

.sb-preview-card:hover {
  background: color-mix(in srgb, var(--app-muted) 50%, transparent);
}

.sb-avatar {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 1.5rem;
  height: 1.5rem;
  border-radius: 50%;
  background: linear-gradient(135deg, var(--app-primary), var(--app-accent));
  color: #fff;
  font-size: 0.5rem;
  font-weight: 700;
  flex-shrink: 0;
}

.sb-avatar--classroom {
  border-radius: 0.375rem;
  background: linear-gradient(135deg, var(--app-accent), var(--app-primary));
}

.sb-preview-title {
  font-size: 0.8125rem;
  font-weight: 600;
  color: var(--app-foreground);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  max-width: 9rem;
  line-height: 1.3;
}

.sb-preview-card:hover .sb-preview-title {
  color: var(--app-primary);
}

.sb-preview-meta {
  display: flex;
  align-items: center;
  gap: 0.25rem;
  font-size: 0.75rem;
  color: var(--app-muted-foreground);
  line-height: 1.2;
}

/* Marquee scroll for overflowing session topics */
.sb-marquee-wrap {
  display: block;
  overflow: hidden;
  white-space: nowrap;
  max-width: 100%;
  mask-image: linear-gradient(to right, black 0%, black 85%, transparent 100%);
  -webkit-mask-image: linear-gradient(to right, black 0%, black 85%, transparent 100%);
}

.sb-marquee-text {
  display: inline-block;
  white-space: nowrap;
}

.sb-preview-card:hover .sb-marquee-text {
  animation: sb-marquee 6s linear 0.4s infinite;
}

@keyframes sb-marquee {
  0%   { transform: translateX(0); }
  100% { transform: translateX(-100%); }
}

/* Status dots & icons */
.sb-status-icon {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 1.125rem;
  height: 1.125rem;
  flex-shrink: 0;
}

.sb-live-dot {
  width: 0.4375rem;
  height: 0.4375rem;
  border-radius: 50%;
  background: #dc2626;
  position: relative;
}

.sb-live-dot::before {
  content: '';
  position: absolute;
  inset: 0;
  border-radius: 50%;
  background: #dc2626;
  animation: sb-ping 1.5s cubic-bezier(0, 0, 0.2, 1) infinite;
}

.sb-starting-dot {
  width: 0.4375rem;
  height: 0.4375rem;
  border-radius: 50%;
  background: #f59e0b;
}

.sb-status-scheduled {
  color: var(--app-muted-foreground);
  opacity: 0.6;
}

.sb-status-ended {
  color: var(--app-muted-foreground);
  opacity: 0.4;
}

.sb-active-dot {
  width: 0.4375rem;
  height: 0.4375rem;
  border-radius: 50%;
  background: #22c55e;
  position: relative;
}

.sb-active-dot::before {
  content: '';
  position: absolute;
  inset: 0;
  border-radius: 50%;
  background: #22c55e;
  animation: sb-ping 1.5s cubic-bezier(0, 0, 0.2, 1) infinite;
}

.sb-unread-badge {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-width: 0.875rem;
  height: 0.875rem;
  padding: 0 0.25rem;
  font-size: 0.5625rem;
  font-weight: 700;
  color: #fff;
  background: var(--app-primary);
  border-radius: 9999px;
  flex-shrink: 0;
}

.sb-view-all {
  display: flex;
  align-items: center;
  gap: 0.25rem;
  padding: 0.25rem 0.5rem;
  margin-top: 0.25rem;
  font-size: 0.75rem;
  font-weight: 600;
  color: var(--app-primary);
  text-decoration: none;
  transition: gap 0.15s, opacity 0.15s;
  opacity: 0.8;
  background: transparent;
  border: none;
  cursor: pointer;
}

.sb-view-all:hover {
  gap: 0.4rem;
  opacity: 1;
}

@keyframes sb-ping {
  75%, 100% {
    transform: scale(2.5);
    opacity: 0;
  }
}
</style>
