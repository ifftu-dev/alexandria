<script setup lang="ts">
import { ref, onMounted, onUnmounted, computed, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useLocalApi } from '@/composables/useLocalApi'
import { useSentinel } from '@/composables/useSentinel'
import { AppButton, AppSpinner, EmptyState, StatusBadge } from '@/components/ui'
import TextContent from '@/components/course/TextContent.vue'
import VideoPlayer from '@/components/course/VideoPlayer.vue'
import QuizEngine from '@/components/course/QuizEngine.vue'
import type { Course, Chapter, Element, Enrollment, ElementProgress, UpdateProgressRequest, QuizResult } from '@/types'

const { invoke } = useLocalApi()
const route = useRoute()
const router = useRouter()

const sentinel = useSentinel()

const courseId = route.params.id as string

const course = ref<Course | null>(null)
const chapters = ref<Chapter[]>([])
const elements = ref<Record<string, Element[]>>({})
const enrollment = ref<Enrollment | null>(null)
const progress = ref<Record<string, ElementProgress>>({})
const loading = ref(true)
const sentinelStarted = ref(false)

const activeChapter = ref<string | null>(null)
const activeElement = ref<string | null>(null)

const currentElement = computed(() => {
  if (!activeChapter.value || !activeElement.value) return null
  return elements.value[activeChapter.value]?.find(e => e.id === activeElement.value) ?? null
})

const isAssessment = computed(() => {
  if (!currentElement.value) return false
  return sentinel.isAssessmentElement(currentElement.value.element_type)
})

// Total progress stats
const totalElements = computed(() => {
  let count = 0
  for (const chElems of Object.values(elements.value)) {
    count += chElems.length
  }
  return count
})

const completedElements = computed(() => {
  let count = 0
  for (const p of Object.values(progress.value)) {
    if (p.status === 'completed') count++
  }
  return count
})

const progressPercent = computed(() => {
  if (totalElements.value === 0) return 0
  return Math.round((completedElements.value / totalElements.value) * 100)
})

onMounted(async () => {
  try {
    const [c, chs, enrollments] = await Promise.all([
      invoke<Course>('get_course', { courseId }),
      invoke<Chapter[]>('list_chapters', { courseId }),
      invoke<Enrollment[]>('list_enrollments'),
    ])
    course.value = c
    chapters.value = chs
    enrollment.value = enrollments.find(e => e.course_id === courseId) ?? null

    // Load elements
    for (const ch of chs) {
      elements.value[ch.id] = await invoke<Element[]>('list_elements', { chapterId: ch.id }).catch(() => [])
    }

    // Load progress if enrolled
    if (enrollment.value) {
      try {
        const p = await invoke<ElementProgress[]>('get_progress', { enrollmentId: enrollment.value.id })
        for (const ep of p) {
          progress.value[ep.element_id] = ep
        }
      } catch { /* no progress yet */ }

      // Start Sentinel monitoring for this enrollment
      await sentinel.start(enrollment.value.id)
      sentinelStarted.value = true
    }

    // Select first element
    const firstCh = chs[0]
    const firstChElems = firstCh ? elements.value[firstCh.id] : undefined
    if (firstCh && firstChElems && firstChElems.length > 0) {
      activeChapter.value = firstCh.id
      activeElement.value = firstChElems[0]!.id
    }
  } catch (e) {
    console.error('Failed to load course:', e)
  } finally {
    loading.value = false
  }
})

// Notify Sentinel when element changes
watch([activeChapter, activeElement], () => {
  if (currentElement.value && sentinelStarted.value) {
    sentinel.setElement(currentElement.value.id, currentElement.value.element_type)
  }
})

onUnmounted(async () => {
  if (sentinelStarted.value) {
    await sentinel.stop()
  }
})

function selectElement(chapterId: string, elementId: string) {
  activeChapter.value = chapterId
  activeElement.value = elementId
}

async function markComplete(score?: number) {
  if (!enrollment.value || !activeElement.value) return
  try {
    const req: UpdateProgressRequest = {
      element_id: activeElement.value,
      status: 'completed',
      score: score ?? null,
    }
    await invoke('update_progress', {
      enrollmentId: enrollment.value.id,
      request: req,
    })
    // Update local progress
    progress.value[activeElement.value] = {
      ...progress.value[activeElement.value],
      id: progress.value[activeElement.value]?.id ?? '',
      enrollment_id: enrollment.value.id,
      element_id: activeElement.value,
      status: 'completed',
      score: score ?? null,
      time_spent: 0,
      completed_at: new Date().toISOString(),
      updated_at: new Date().toISOString(),
    }
    // Auto-advance to next element
    advanceToNext()
  } catch (e) {
    console.error('Failed to update progress:', e)
  }
}

function onQuizComplete(result: QuizResult) {
  // Send score so the evidence pipeline triggers
  markComplete(result.score)
}

function onVideoComplete() {
  markComplete()
}

function advanceToNext() {
  if (!activeChapter.value || !activeElement.value) return
  const chElems = elements.value[activeChapter.value]
  if (!chElems) return
  const idx = chElems.findIndex(e => e.id === activeElement.value)
  if (idx >= 0 && idx < chElems.length - 1) {
    const nextEl = chElems[idx + 1]
    if (nextEl) {
      activeElement.value = nextEl.id
      return
    }
  }
  const chIdx = chapters.value.findIndex(c => c.id === activeChapter.value)
  if (chIdx >= 0 && chIdx < chapters.value.length - 1) {
    const nextCh = chapters.value[chIdx + 1]
    if (!nextCh) return
    const nextElems = elements.value[nextCh.id]
    if (nextElems && nextElems.length > 0) {
      activeChapter.value = nextCh.id
      activeElement.value = nextElems[0]!.id
    }
  }
}

function elementStatus(elementId: string): string {
  return progress.value[elementId]?.status ?? 'not_started'
}

function elementTypeIcon(elementType: string): string {
  switch (elementType) {
    case 'video': return 'M14.752 11.168l-3.197-2.132A1 1 0 0010 9.87v4.263a1 1 0 001.555.832l3.197-2.132a1 1 0 000-1.664z'
    case 'text': return 'M4 6h16M4 12h16M4 18h7'
    case 'quiz': case 'assessment': return 'M9 5H7a2 2 0 00-2 2v12a2 2 0 002 2h10a2 2 0 002-2V7a2 2 0 00-2-2h-2M9 5a2 2 0 002 2h2a2 2 0 002-2M9 5a2 2 0 012-2h2a2 2 0 012 2'
    case 'interactive': return 'M13 10V3L4 14h7v7l9-11h-7z'
    default: return 'M7 21h10a2 2 0 002-2V9.414a1 1 0 00-.293-.707l-5.414-5.414A1 1 0 0012.586 3H7a2 2 0 00-2 2v14a2 2 0 002 2z'
  }
}
</script>

<template>
  <div>
    <AppSpinner v-if="loading" label="Loading course..." />

    <EmptyState v-else-if="!course" title="Course not found" />

    <div v-else class="flex gap-0 h-[calc(100vh-8rem)]">
      <!-- Sidebar: Chapter/Element navigation -->
      <div class="w-72 shrink-0 overflow-y-auto border-r border-[rgb(var(--color-border))] p-4 space-y-4">
        <button
          class="text-sm text-[rgb(var(--color-muted-foreground))] hover:text-[rgb(var(--color-foreground))] transition-colors flex items-center gap-1"
          @click="router.push(`/courses/${courseId}`)"
        >
          <svg class="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
            <path stroke-linecap="round" stroke-linejoin="round" d="M15 19l-7-7 7-7" />
          </svg>
          Back to course
        </button>

        <div>
          <h2 class="text-sm font-semibold mb-1">{{ course.title }}</h2>
          <!-- Progress bar -->
          <div class="flex items-center gap-2 mb-1">
            <div class="flex-1 h-1.5 bg-[rgb(var(--color-muted)/0.3)] rounded-full overflow-hidden">
              <div
                class="h-full bg-[rgb(var(--color-success))] transition-all duration-500"
                :style="{ width: `${progressPercent}%` }"
              />
            </div>
            <span class="text-xs text-[rgb(var(--color-muted-foreground))] whitespace-nowrap">
              {{ completedElements }}/{{ totalElements }}
            </span>
          </div>
        </div>

        <!-- Sentinel indicator -->
        <div
          v-if="sentinel.isActive.value"
          class="flex items-center gap-2 px-2 py-1.5 rounded text-xs bg-[rgb(var(--color-muted)/0.2)]"
        >
          <span class="relative flex h-2 w-2">
            <span class="animate-ping absolute inline-flex h-full w-full rounded-full opacity-75"
              :class="sentinel.integrityScore.value > 0.7 ? 'bg-[rgb(var(--color-success))]' : sentinel.integrityScore.value > 0.4 ? 'bg-amber-400' : 'bg-[rgb(var(--color-destructive))]'"
            />
            <span class="relative inline-flex rounded-full h-2 w-2"
              :class="sentinel.integrityScore.value > 0.7 ? 'bg-[rgb(var(--color-success))]' : sentinel.integrityScore.value > 0.4 ? 'bg-amber-400' : 'bg-[rgb(var(--color-destructive))]'"
            />
          </span>
          <span class="text-[rgb(var(--color-muted-foreground))]">
            Sentinel {{ Math.round(sentinel.integrityScore.value * 100) }}%
          </span>
        </div>

        <!-- Chapter list -->
        <div v-for="ch in chapters" :key="ch.id" class="space-y-0.5">
          <div class="text-xs font-medium text-[rgb(var(--color-muted-foreground))] uppercase tracking-wider px-2 py-1">
            {{ ch.title }}
          </div>
          <button
            v-for="el in elements[ch.id] ?? []"
            :key="el.id"
            class="flex items-center gap-2 w-full p-2 rounded text-sm transition-colors"
            :class="activeElement === el.id
              ? 'bg-[rgb(var(--color-primary)/0.1)] text-[rgb(var(--color-primary))] font-medium'
              : 'text-[rgb(var(--color-muted-foreground))] hover:bg-[rgb(var(--color-muted)/0.5)]'"
            @click="selectElement(ch.id, el.id)"
          >
            <span
              class="w-5 h-5 rounded-full border flex items-center justify-center shrink-0"
              :class="elementStatus(el.id) === 'completed'
                ? 'bg-[rgb(var(--color-success))] border-[rgb(var(--color-success))] text-white'
                : 'border-[rgb(var(--color-border))]'"
            >
              <svg v-if="elementStatus(el.id) === 'completed'" class="w-2.5 h-2.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="3">
                <path stroke-linecap="round" stroke-linejoin="round" d="M5 13l4 4L19 7" />
              </svg>
              <svg v-else class="w-2.5 h-2.5 text-[rgb(var(--color-muted-foreground)/0.5)]" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                <path stroke-linecap="round" stroke-linejoin="round" :d="elementTypeIcon(el.element_type)" />
              </svg>
            </span>
            <span class="truncate text-left">{{ el.title }}</span>
          </button>
        </div>
      </div>

      <!-- Main content area -->
      <div class="flex-1 overflow-y-auto p-6">
        <div v-if="currentElement" class="max-w-3xl mx-auto">
          <!-- Element header -->
          <div class="flex items-center gap-2 mb-2">
            <StatusBadge :status="currentElement.element_type" />
            <span v-if="currentElement.duration_seconds" class="text-xs text-[rgb(var(--color-muted-foreground))]">
              {{ Math.round(currentElement.duration_seconds / 60) }} min
            </span>
            <span v-if="isAssessment" class="text-xs font-medium text-amber-600 dark:text-amber-400 bg-amber-100 dark:bg-amber-900/30 px-2 py-0.5 rounded">
              Monitored
            </span>
          </div>
          <h1 class="text-lg font-bold mb-6">{{ currentElement.title }}</h1>

          <!-- Content renderers based on element_type -->
          <div class="mb-6">
            <!-- Video -->
            <VideoPlayer
              v-if="currentElement.element_type === 'video'"
              :content-cid="currentElement.content_cid"
              :title="currentElement.title"
              @complete="onVideoComplete"
            />

            <!-- Text/Reading -->
            <TextContent
              v-else-if="currentElement.element_type === 'text'"
              :content-cid="currentElement.content_cid"
              @complete="markComplete()"
            />

            <!-- Quiz / Assessment -->
            <QuizEngine
              v-else-if="currentElement.element_type === 'quiz' || currentElement.element_type === 'assessment'"
              :content-cid="currentElement.content_cid"
              :element-id="currentElement.id"
              @complete="onQuizComplete"
            />

            <!-- Interactive (placeholder with content rendering) -->
            <div v-else-if="currentElement.element_type === 'interactive'" class="space-y-4">
              <TextContent
                :content-cid="currentElement.content_cid"
              />
              <div class="text-xs text-[rgb(var(--color-muted-foreground))] italic">
                Interactive simulation support coming in a future update.
              </div>
            </div>

            <!-- Fallback for unknown types -->
            <div v-else class="card p-8 text-center">
              <div v-if="currentElement.content_cid" class="text-sm text-[rgb(var(--color-muted-foreground))]">
                <p class="mb-2">Content CID:</p>
                <code class="font-mono text-xs break-all">{{ currentElement.content_cid }}</code>
              </div>
              <div v-else class="text-sm text-[rgb(var(--color-muted-foreground))]">
                No content attached to this element yet.
              </div>
            </div>
          </div>

          <!-- Actions -->
          <div v-if="enrollment" class="flex items-center gap-3 pt-4 border-t border-[rgb(var(--color-border))]">
            <AppButton
              v-if="elementStatus(currentElement.id) !== 'completed' && currentElement.element_type !== 'quiz' && currentElement.element_type !== 'assessment'"
              @click="markComplete()"
            >
              Mark as Complete
            </AppButton>
            <div v-if="elementStatus(currentElement.id) === 'completed'" class="flex items-center gap-2 text-sm text-[rgb(var(--color-success))]">
              <svg class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                <path stroke-linecap="round" stroke-linejoin="round" d="M5 13l4 4L19 7" />
              </svg>
              Completed
              <span v-if="progress[currentElement.id]?.score != null" class="text-xs text-[rgb(var(--color-muted-foreground))]">
                ({{ Math.round((progress[currentElement.id]!.score!) * 100) }}%)
              </span>
            </div>
            <AppButton
              v-if="elementStatus(currentElement.id) === 'completed'"
              variant="secondary"
              size="sm"
              @click="advanceToNext"
            >
              Next
            </AppButton>
          </div>
        </div>

        <EmptyState
          v-else
          title="No element selected"
          description="Select an element from the sidebar to start learning."
        />
      </div>
    </div>
  </div>
</template>
