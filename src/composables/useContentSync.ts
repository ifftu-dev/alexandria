import { readonly, ref } from 'vue'
import { onProfileLocked } from './useProfiles'

type ContentSyncPhase = 'idle' | 'running' | 'success' | 'error'

interface ContentSyncStats {
  hydrated: number
  beforeCourses: number
  afterCourses: number
  newCourses: number
  durationMs: number
}

const phase = ref<ContentSyncPhase>('idle')
const stats = ref<ContentSyncStats | null>(null)
const error = ref<string | null>(null)
const visible = ref(false)

let hideTimer: ReturnType<typeof setTimeout> | null = null
let profileGeneration = 0
let activeSyncGeneration: number | null = null

function clearHideTimer() {
  if (!hideTimer) return
  clearTimeout(hideTimer)
  hideTimer = null
}

function startContentSync() {
  clearHideTimer()
  activeSyncGeneration = profileGeneration
  phase.value = 'running'
  error.value = null
  visible.value = true
}

function completeContentSync(payload: {
  hydrated: number
  beforeCourses: number
  afterCourses: number
  durationMs: number
}) {
  if (activeSyncGeneration !== profileGeneration) return
  clearHideTimer()
  const newCourses = Math.max(0, payload.afterCourses - payload.beforeCourses)
  stats.value = {
    hydrated: payload.hydrated,
    beforeCourses: payload.beforeCourses,
    afterCourses: payload.afterCourses,
    newCourses,
    durationMs: payload.durationMs,
  }
  phase.value = 'success'
  error.value = null
  visible.value = true
  hideTimer = setTimeout(() => {
    visible.value = false
  }, 20000)
}

function failContentSync(message: string) {
  if (activeSyncGeneration !== profileGeneration) return
  clearHideTimer()
  phase.value = 'error'
  error.value = message
  visible.value = true
  hideTimer = setTimeout(() => {
    visible.value = false
  }, 25000)
}

onProfileLocked(() => {
  profileGeneration += 1
  activeSyncGeneration = null
  clearHideTimer()
  phase.value = 'idle'
  stats.value = null
  error.value = null
  visible.value = false
})

export function useContentSync() {
  return {
    phase: readonly(phase),
    stats: readonly(stats),
    error: readonly(error),
    visible: readonly(visible),
    startContentSync,
    completeContentSync,
    failContentSync,
  }
}
