<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
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
      'sb-root',
      collapsed ? 'sb-root--collapsed' : 'sb-root--expanded',
    ]"
  >
    <!-- Scrollable nav area -->
    <nav class="sb-nav">

      <!-- ═══════════════════════════════════════ -->
      <!-- Primary nav — flat items                -->
      <!-- ═══════════════════════════════════════ -->
      <div class="sb-group">
        <button :class="['sb-item', { 'sb-item--active': isActive('/home') }]" :title="collapsed ? 'Home' : undefined" @click="navigate('/home')">
          <svg class="sb-icon" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.75">
            <path stroke-linecap="round" stroke-linejoin="round" d="M3 12l2-2m0 0l7-7 7 7M5 10v10a1 1 0 001 1h3m10-11l2 2m-2-2v10a1 1 0 01-1 1h-3m-6 0a1 1 0 001-1v-4a1 1 0 011-1h2a1 1 0 011 1v4a1 1 0 001 1m-6 0h6" />
          </svg>
          <span class="sb-label">Home</span>
          <div v-if="false" class="sb-tooltip">Home</div>
        </button>

        <button :class="['sb-item', { 'sb-item--active': isActive('/opinions') }]" :title="collapsed ? 'Opinions' : undefined" @click="navigate('/opinions')">
          <svg class="sb-icon" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.75">
            <path stroke-linecap="round" stroke-linejoin="round" d="M7 8h10M7 12h4m1 8l-4-4H5a2 2 0 01-2-2V6a2 2 0 012-2h14a2 2 0 012 2v8a2 2 0 01-2 2h-3l-4 4z" />
          </svg>
          <span class="sb-label">Opinions</span>
          <div v-if="false" class="sb-tooltip">Opinions</div>
        </button>

        <button :class="['sb-item', { 'sb-item--active': isActive('/governance') }]" :title="collapsed ? 'Governance' : undefined" @click="navigate('/governance')">
          <svg class="sb-icon" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.75">
            <path stroke-linecap="round" stroke-linejoin="round" d="M3 6l3 1m0 0l-3 9a5.002 5.002 0 006.001 0M6 7l3 9M6 7l6-2m6 2l3-1m-3 1l-3 9a5.002 5.002 0 006.001 0M18 7l3 9m-3-9l-6-2m0-2v2m0 16V5m0 16H9m3 0h3" />
          </svg>
          <span class="sb-label">Governance</span>
          <div v-if="false" class="sb-tooltip">Governance</div>
        </button>

        <button :class="['sb-item', { 'sb-item--active': isActive('/skills') }]" :title="collapsed ? 'Skills' : undefined" @click="navigate('/skills')">
          <svg class="sb-icon" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.75">
            <path stroke-linecap="round" stroke-linejoin="round" d="M3.75 6A2.25 2.25 0 016 3.75h2.25A2.25 2.25 0 0110.5 6v2.25a2.25 2.25 0 01-2.25 2.25H6a2.25 2.25 0 01-2.25-2.25V6zM3.75 15.75A2.25 2.25 0 016 13.5h2.25a2.25 2.25 0 012.25 2.25V18a2.25 2.25 0 01-2.25 2.25H6A2.25 2.25 0 013.75 18v-2.25zM13.5 6a2.25 2.25 0 012.25-2.25H18A2.25 2.25 0 0120.25 6v2.25A2.25 2.25 0 0118 10.5h-2.25a2.25 2.25 0 01-2.25-2.25V6zM13.5 15.75a2.25 2.25 0 012.25-2.25H18a2.25 2.25 0 012.25 2.25V18A2.25 2.25 0 0118 20.25h-2.25A2.25 2.25 0 0113.5 18v-2.25z" />
          </svg>
          <span class="sb-label">Skills</span>
          <div v-if="false" class="sb-tooltip">Skills</div>
        </button>
      </div>

      <div class="sb-separator" />

      <!-- ═══════════════════════════════════════ -->
      <!-- Live Tutoring — collapsible previews    -->
      <!-- ═══════════════════════════════════════ -->
      <div class="sb-group">
        <!-- Collapsed: icon-only -->
        <button
          v-if="collapsed"
          :class="['sb-item', { 'sb-item--active': isActive('/tutoring') }]"
          title="Live Tutoring"
          @click="navigate('/tutoring')"
        >
          <svg class="sb-icon" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.75">
            <path stroke-linecap="round" stroke-linejoin="round" d="M15 10l4.553-2.276A1 1 0 0121 8.618v6.764a1 1 0 01-1.447.894L15 14M5 18h8a2 2 0 002-2V8a2 2 0 00-2-2H5a2 2 0 00-2 2v8a2 2 0 002 2z" />
          </svg>
          <div class="sb-tooltip">Live Tutoring</div>
        </button>

        <!-- Expanded: section header + chevron -->
        <div v-if="!collapsed" class="sb-section-header">
          <button class="sb-section-title" @click="navigate('/tutoring')">Live Tutoring</button>
          <button class="sb-section-chevron" @click.stop="toggleSection('tutoring')">
            <svg :class="['h-3.5 w-3.5 transition-transform duration-200', isSectionOpen('tutoring') ? '' : '-rotate-90']" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2.5">
              <path stroke-linecap="round" stroke-linejoin="round" d="M19 9l-7 7-7-7" />
            </svg>
          </button>
        </div>

        <!-- Inline previews -->
        <Transition
          enter-active-class="transition-all duration-200 ease-out"
          enter-from-class="max-h-0 opacity-0"
          enter-to-class="max-h-96 opacity-100"
          leave-active-class="transition-all duration-150 ease-in"
          leave-from-class="max-h-96 opacity-100"
          leave-to-class="max-h-0 opacity-0"
        >
          <div v-if="isSectionOpen('tutoring') && !collapsed" class="sb-preview-list">
            <template v-if="tutoringPreviews.length > 0">
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
                <span v-if="session.status === 'active'" class="sb-status-icon" title="Active"><span class="sb-live-dot" style="background: #22c55e" /></span>
                <span v-else-if="session.status === 'ended'" class="sb-status-icon sb-status-ended" title="Ended">
                  <svg class="h-3 w-3" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2.5"><path stroke-linecap="round" stroke-linejoin="round" d="M5 13l4 4L19 7" /></svg>
                </span>
                <span v-else class="sb-status-icon sb-status-ended" title="Cancelled">
                  <svg class="h-3 w-3" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2.5"><path stroke-linecap="round" stroke-linejoin="round" d="M6 18L18 6M6 6l12 12" /></svg>
                </span>
              </button>
              <button class="sb-view-all" @click="navigate('/tutoring')">
                View all sessions
                <svg class="h-3 w-3" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2.5"><path stroke-linecap="round" stroke-linejoin="round" d="M9 5l7 7-7 7" /></svg>
              </button>
            </template>
            <p v-else class="sb-empty-hint">No sessions yet</p>
          </div>
        </Transition>
      </div>

      <div class="sb-separator" />

      <!-- ═══════════════════════════════════════ -->
      <!-- Classrooms — collapsible previews       -->
      <!-- ═══════════════════════════════════════ -->
      <div class="sb-group">
        <!-- Collapsed: icon-only -->
        <button
          v-if="collapsed"
          :class="['sb-item', { 'sb-item--active': isActive('/classrooms') }]"
          title="Classrooms"
          @click="navigate('/classrooms')"
        >
          <svg class="sb-icon" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.75">
            <path stroke-linecap="round" stroke-linejoin="round" d="M18 18.72a9.094 9.094 0 003.741-.479 3 3 0 00-4.682-2.72m.94 3.198l.001.031c0 .225-.012.447-.037.666A11.944 11.944 0 0112 21c-2.17 0-4.207-.576-5.963-1.584A6.062 6.062 0 016 18.719m12 0a5.971 5.971 0 00-.941-3.197m0 0A5.995 5.995 0 0012 12.75a5.995 5.995 0 00-5.058 2.772m0 0a3 3 0 00-4.681 2.72 8.986 8.986 0 003.74.477m.94-3.197a5.971 5.971 0 00-.94 3.197M15 6.75a3 3 0 11-6 0 3 3 0 016 0zm6 3a2.25 2.25 0 11-4.5 0 2.25 2.25 0 014.5 0zm-13.5 0a2.25 2.25 0 11-4.5 0 2.25 2.25 0 014.5 0z" />
          </svg>
          <div class="sb-tooltip">Classrooms</div>
        </button>

        <!-- Expanded: section header + chevron -->
        <div v-if="!collapsed" class="sb-section-header">
          <button class="sb-section-title" @click="navigate('/classrooms')">Classrooms</button>
          <button class="sb-section-chevron" @click.stop="toggleSection('classrooms')">
            <svg :class="['h-3.5 w-3.5 transition-transform duration-200', isSectionOpen('classrooms') ? '' : '-rotate-90']" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2.5">
              <path stroke-linecap="round" stroke-linejoin="round" d="M19 9l-7 7-7-7" />
            </svg>
          </button>
        </div>

        <!-- Inline previews -->
        <Transition
          enter-active-class="transition-all duration-200 ease-out"
          enter-from-class="max-h-0 opacity-0"
          enter-to-class="max-h-96 opacity-100"
          leave-active-class="transition-all duration-150 ease-in"
          leave-from-class="max-h-96 opacity-100"
          leave-to-class="max-h-0 opacity-0"
        >
          <div v-if="isSectionOpen('classrooms') && !collapsed" class="sb-preview-list">
            <template v-if="classroomPreviews.length > 0">
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
            </template>
            <p v-else class="sb-empty-hint">No classrooms yet</p>
          </div>
        </Transition>
      </div>

    </nav>

    <!-- Skill Graph widget -->
    <!-- Collapsed: icon-only with tooltip -->
    <div v-if="collapsed" class="sb-skill-graph-collapsed">
      <button class="sb-item" title="Skill Graph" @click="navigate('/skills')">
        <svg class="sb-icon" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.75">
          <path stroke-linecap="round" stroke-linejoin="round" d="M3.75 6A2.25 2.25 0 016 3.75h2.25A2.25 2.25 0 0110.5 6v2.25a2.25 2.25 0 01-2.25 2.25H6a2.25 2.25 0 01-2.25-2.25V6zM3.75 15.75A2.25 2.25 0 016 13.5h2.25a2.25 2.25 0 012.25 2.25V18a2.25 2.25 0 01-2.25 2.25H6A2.25 2.25 0 013.75 18v-2.25zM13.5 6a2.25 2.25 0 012.25-2.25H18A2.25 2.25 0 0120.25 6v2.25A2.25 2.25 0 0118 10.5h-2.25a2.25 2.25 0 01-2.25-2.25V6zM13.5 15.75a2.25 2.25 0 012.25-2.25H18a2.25 2.25 0 012.25 2.25V18A2.25 2.25 0 0118 20.25h-2.25A2.25 2.25 0 0113.5 18v-2.25z" />
        </svg>
      </button>
    </div>
    <!-- Expanded: full skill graph widget -->
    <div v-else class="sb-skill-graph">
      <SidebarSkillGraph />
    </div>

  </aside>
</template>

<style scoped>
/* ═══════════════════════════════════════════
   Root layout
   ═══════════════════════════════════════════ */

.sb-root {
  position: relative;
  display: flex;
  height: 100%;
  flex-direction: column;
  overflow: hidden;
  border-right: 1px solid var(--app-border);
  background: var(--app-background);
  transition: width 0.3s cubic-bezier(0.22, 1, 0.36, 1);
}

.sb-root--expanded { width: 15rem; }   /* 240px — slightly tighter than before */
.sb-root--collapsed { width: 4rem; }

.sb-nav {
  flex: 1;
  overflow-y: auto;
  overflow-x: hidden;
  padding: 0.75rem 0.5rem;
  display: flex;
  flex-direction: column;
  gap: 0.125rem;                          /* tight gap between groups */
}

/* ═══════════════════════════════════════════
   Groups + separator
   ═══════════════════════════════════════════ */

.sb-group {
  display: flex;
  flex-direction: column;
  gap: 0.0625rem;                         /* 1px visual gap between items */
}

.sb-separator {
  height: 1px;
  margin: 0.5rem 0;
  background: var(--app-border);
  opacity: 0.5;
}

/* ═══════════════════════════════════════════
   Nav items — uniform height, icon, label
   YouTube-style: rounded-lg, 10px radius,
   icon 20px, label 14px/500
   ═══════════════════════════════════════════ */

.sb-item {
  position: relative;
  display: flex;
  align-items: center;
  gap: 0.75rem;
  width: 100%;
  padding: 0.5rem 0.75rem;
  border: none;
  border-radius: 0.625rem;               /* 10px — YouTube uses this */
  background: transparent;
  color: var(--app-muted-foreground);
  font-size: 0.875rem;                   /* 14px */
  font-weight: 500;
  line-height: 1;
  cursor: pointer;
  transition: background 0.15s, color 0.15s;
  text-align: left;
}

.sb-item:hover {
  background: var(--app-muted);
  color: var(--app-foreground);
}

.sb-item--active {
  background: color-mix(in srgb, var(--app-muted) 80%, transparent);
  color: var(--app-foreground);
  font-weight: 600;
}

.sb-icon {
  width: 1.25rem;                         /* 20px uniform */
  height: 1.25rem;
  flex-shrink: 0;
}

.sb-label {
  white-space: nowrap;
  transition: opacity 0.3s, width 0.3s;
}

.sb-root--collapsed .sb-label {
  opacity: 0;
  width: 0;
  overflow: hidden;
}

/* ═══════════════════════════════════════════
   Section headers (Tutoring, Classrooms)
   Lighter, lowercase — doesn't compete
   with nav items
   ═══════════════════════════════════════════ */

.sb-section-header {
  display: flex;
  align-items: center;
  padding: 0.375rem 0.75rem 0.25rem;
}

.sb-section-title {
  flex: 1;
  font-size: 0.6875rem;                  /* 11px */
  font-weight: 600;
  letter-spacing: 0.04em;
  text-transform: uppercase;
  color: var(--app-muted-foreground);
  opacity: 0.7;
  background: none;
  border: none;
  cursor: pointer;
  text-align: left;
  padding: 0;
  transition: opacity 0.15s;
}

.sb-section-title:hover { opacity: 1; }

.sb-section-chevron {
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 0.125rem;
  border-radius: 0.25rem;
  background: none;
  border: none;
  color: var(--app-muted-foreground);
  opacity: 0.5;
  cursor: pointer;
  transition: background 0.15s, opacity 0.15s;
}

.sb-section-chevron:hover {
  background: var(--app-muted);
  opacity: 0.8;
}

/* ═══════════════════════════════════════════
   Inline preview cards
   ═══════════════════════════════════════════ */

.sb-preview-list {
  padding: 0.125rem 0.375rem 0.375rem;
  display: flex;
  flex-direction: column;
  gap: 0.125rem;
  overflow: hidden;
}

.sb-preview-card {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  padding: 0.375rem 0.5rem;
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
  font-weight: 500;
  color: var(--app-foreground);
  line-height: 1.3;
}

.sb-preview-title-slot {
  min-width: 0;
  overflow: hidden;
}

.sb-preview-card:hover .sb-preview-title {
  color: var(--app-primary);
}

.sb-preview-meta {
  font-size: 0.6875rem;
  color: var(--app-muted-foreground);
  line-height: 1.2;
}

/* ═══════════════════════════════════════════
   Status indicators
   ═══════════════════════════════════════════ */

.sb-status-icon {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 1rem;
  height: 1rem;
  flex-shrink: 0;
}

.sb-live-dot {
  width: 0.375rem;
  height: 0.375rem;
  border-radius: 50%;
  position: relative;
}

.sb-live-dot::before {
  content: '';
  position: absolute;
  inset: 0;
  border-radius: 50%;
  background: inherit;
  animation: sb-ping 1.5s cubic-bezier(0, 0, 0.2, 1) infinite;
}

.sb-status-ended {
  color: var(--app-muted-foreground);
  opacity: 0.4;
}

/* ═══════════════════════════════════════════
   "View all" link
   ═══════════════════════════════════════════ */

.sb-view-all {
  display: flex;
  align-items: center;
  gap: 0.25rem;
  padding: 0.25rem 0.5rem;
  margin-top: 0.125rem;
  font-size: 0.75rem;
  font-weight: 500;
  color: var(--app-primary);
  opacity: 0.7;
  background: none;
  border: none;
  cursor: pointer;
  transition: gap 0.15s, opacity 0.15s;
}

.sb-view-all:hover {
  gap: 0.4rem;
  opacity: 1;
}

.sb-empty-hint {
  font-size: 0.75rem;
  color: var(--app-muted-foreground);
  opacity: 0.6;
  padding: 0.25rem 0.5rem;
  margin: 0;
}

/* ═══════════════════════════════════════════
   Collapsed tooltip
   ═══════════════════════════════════════════ */

.sb-tooltip {
  position: absolute;
  left: 100%;
  top: 50%;
  transform: translateY(-50%);
  margin-left: 0.625rem;
  padding: 0.3rem 0.625rem;
  font-size: 0.75rem;
  font-weight: 500;
  white-space: nowrap;
  color: var(--app-background);
  background: var(--app-foreground);
  border-radius: 0.375rem;
  box-shadow: 0 4px 12px rgb(0 0 0 / 0.15);
  z-index: 70;
  pointer-events: none;
  opacity: 0;
  transition: opacity 0.15s;
}

.sb-item:hover .sb-tooltip { opacity: 1; }

/* ═══════════════════════════════════════════
   Skill Graph widget
   ═══════════════════════════════════════════ */

.sb-skill-graph {
  overflow: hidden;
  border-top: 1px solid var(--app-border);
  padding: 0.5rem;
  max-height: 16rem;
  opacity: 1;
}

.sb-skill-graph-collapsed {
  border-top: 1px solid var(--app-border);
  padding: 0.25rem 0.5rem;
  overflow: hidden;
}

/* ═══════════════════════════════════════════
   Animations
   ═══════════════════════════════════════════ */

@keyframes sb-ping {
  75%, 100% {
    transform: scale(2.5);
    opacity: 0;
  }
}
</style>
