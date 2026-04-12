<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue'
import { useRouter, useRoute } from 'vue-router'
import SidebarSkillGraph from '@/components/layout/SidebarSkillGraph.vue'
import TickerText from '@/components/layout/TickerText.vue'
import { useTutoringRoom } from '@/composables/useTutoringRoom'
import { useClassroom } from '@/composables/useClassroom'

defineProps<{ collapsed: boolean }>()
const emit = defineEmits<{ toggle: [] }>()
const router = useRouter()
const route = useRoute()
const { sessions: tutoringSessionsList, refreshSessions } = useTutoringRoom()

// Keyboard shortcut: Cmd+\ (macOS) / Ctrl+\ (Windows)
function onKeyDown(e: KeyboardEvent) {
  if ((e.metaKey || e.ctrlKey) && e.key === '\\') {
    e.preventDefault()
    emit('toggle')
  }
}

onMounted(() => {
  document.addEventListener('keydown', onKeyDown)
})

onUnmounted(() => {
  document.removeEventListener('keydown', onKeyDown)
})

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
// Tutoring sessions (real data from backend)
// =========================================
const tutoringPreviews = ref<{ id: string; title: string; initials: string; status: string }[]>([])

onMounted(async () => {
  await refreshSessions()
  updateTutoringPreviews()
})

function updateTutoringPreviews() {
  tutoringPreviews.value = tutoringSessionsList.value.slice(0, 4).map(s => ({
    id: s.id,
    title: s.title,
    initials: s.title.split(' ').slice(0, 2).map(w => w[0] || '').join('').toUpperCase() || 'T',
    status: s.status,
  }))
}

// =========================================
// Classrooms (real data)
// =========================================
const { classrooms, loadClassrooms } = useClassroom()

onMounted(async () => {
  try { await loadClassrooms() } catch { /* not logged in yet */ }
})

const classroomPreviews = computed(() =>
  classrooms.value.slice(0, 4).map(c => ({
    id: c.id,
    name: c.name,
    initial: (c.icon_emoji ?? c.name.charAt(0)).toUpperCase(),
    member_count: c.member_count,
  }))
)
</script>

<template>
  <aside
    :class="[
      'relative flex h-full flex-col overflow-hidden border-r border-border bg-background transition-all duration-300',
      collapsed ? 'w-16' : 'w-64',
    ]"
  >
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
            @click="navigate('/tutoring')"
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
              @click="navigate('/tutoring')"
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
              @click="navigate(`/tutoring/${session.id}`)"
            >
              <div class="sb-avatar">{{ session.initials }}</div>
              <div class="min-w-0 flex-1 sb-preview-title-slot">
                <TickerText class="sb-preview-title" :text="session.title" />
              </div>
              <!-- Active: pulsing green dot -->
              <span v-if="session.status === 'active'" class="sb-status-icon" title="Active"><span class="sb-live-dot" style="background: #22c55e" /><span class="sb-live-dot-ping" style="background: #22c55e" /></span>
              <!-- Ended: checkmark -->
              <span v-else-if="session.status === 'ended'" class="sb-status-icon sb-status-ended" title="Ended">
                <svg class="h-3 w-3" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2.5"><path stroke-linecap="round" stroke-linejoin="round" d="M5 13l4 4L19 7" /></svg>
              </span>
              <!-- Cancelled: X -->
              <span v-else class="sb-status-icon sb-status-ended" title="Cancelled">
                <svg class="h-3 w-3" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2.5"><path stroke-linecap="round" stroke-linejoin="round" d="M6 18L18 6M6 6l12 12" /></svg>
              </span>
            </button>

            <button class="sb-view-all" @click="navigate('/tutoring')">
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
            @click="navigate('/classrooms')"
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
              @click="navigate('/classrooms')"
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
              @click="navigate(`/classrooms/${classroom.id}`)"
            >
              <div class="sb-avatar sb-avatar--classroom">{{ classroom.initial }}</div>
              <div class="min-w-0 flex-1 sb-preview-title-slot">
                <TickerText class="sb-preview-title" :text="classroom.name" />
                <span class="sb-preview-meta">{{ classroom.member_count }} members</span>
              </div>
            </button>

            <button class="sb-view-all" @click="navigate('/classrooms')">
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

  </aside>
</template>

<style scoped>
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

/* Title wraps the TickerText root. The ticker itself owns the
   overflow-clipping and animation — these rules only set typography
   and hover state. Width is constrained by the flex-1 slot above. */
.sb-preview-title {
  font-size: 0.8125rem;
  font-weight: 600;
  color: var(--app-foreground);
  line-height: 1.3;
}

.sb-preview-title-slot {
  /* Guard-rail: ensure the slot itself doesn't grow past the parent
     flex container when the status icon / chevron is present. */
  min-width: 0;
  overflow: hidden;
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
