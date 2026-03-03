import { computed, readonly, ref } from 'vue'

type ContentSyncPhase = 'idle' | 'running' | 'success' | 'error'

interface ContentSyncStats {
  bootstrapped: number
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

function clearHideTimer() {
  if (!hideTimer) return
  clearTimeout(hideTimer)
  hideTimer = null
}

function startContentSync() {
  clearHideTimer()
  phase.value = 'running'
  error.value = null
  visible.value = true
}

function completeContentSync(payload: {
  bootstrapped: number
  hydrated: number
  beforeCourses: number
  afterCourses: number
  durationMs: number
}) {
  clearHideTimer()
  const newCourses = Math.max(0, payload.afterCourses - payload.beforeCourses)
  stats.value = {
    bootstrapped: payload.bootstrapped,
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
  clearHideTimer()
  phase.value = 'error'
  error.value = message
  visible.value = true
  hideTimer = setTimeout(() => {
    visible.value = false
  }, 25000)
}

const statusMessage = computed(() => {
  if (phase.value === 'running') {
    return 'Content sync: checking for new courses...'
  }
  if (phase.value === 'success' && stats.value) {
    return `Content sync complete: +${stats.value.newCourses} courses | hydrated ${stats.value.hydrated} | bootstrap ${stats.value.bootstrapped} | ${stats.value.durationMs}ms`
  }
  if (phase.value === 'error' && error.value) {
    return `Content sync failed: ${error.value}`
  }
  return ''
})

export function useContentSync() {
  return {
    phase: readonly(phase),
    stats: readonly(stats),
    error: readonly(error),
    visible: readonly(visible),
    statusMessage,
    startContentSync,
    completeContentSync,
    failContentSync,
  }
}
