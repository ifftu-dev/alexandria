<script setup lang="ts">
import { ref, onMounted, computed } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useLocalApi } from '@/composables/useLocalApi'
import { AppButton, AppSpinner, EmptyState, StatusBadge } from '@/components/ui'
import type { Course, Chapter, Element, Enrollment, ElementProgress, UpdateProgressRequest } from '@/types'

const { invoke } = useLocalApi()
const route = useRoute()
const router = useRouter()

const courseId = route.params.id as string

const course = ref<Course | null>(null)
const chapters = ref<Chapter[]>([])
const elements = ref<Record<string, Element[]>>({})
const enrollment = ref<Enrollment | null>(null)
const progress = ref<Record<string, ElementProgress>>({})
const loading = ref(true)

const activeChapter = ref<string | null>(null)
const activeElement = ref<string | null>(null)

const currentElement = computed(() => {
  if (!activeChapter.value || !activeElement.value) return null
  return elements.value[activeChapter.value]?.find(e => e.id === activeElement.value) ?? null
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

function selectElement(chapterId: string, elementId: string) {
  activeChapter.value = chapterId
  activeElement.value = elementId
}

async function markComplete() {
  if (!enrollment.value || !activeElement.value) return
  try {
    const req: UpdateProgressRequest = {
      element_id: activeElement.value,
      status: 'completed',
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
      score: null,
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
  // Try next chapter
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
</script>

<template>
  <div>
    <AppSpinner v-if="loading" label="Loading course..." />

    <EmptyState v-else-if="!course" title="Course not found" />

    <div v-else class="flex gap-6 h-[calc(100vh-8rem)]">
      <!-- Sidebar: Chapter/Element navigation -->
      <div class="w-64 shrink-0 overflow-y-auto border-r border-[rgb(var(--color-border))] pr-4">
        <button
          class="text-sm text-[rgb(var(--color-muted-foreground))] hover:text-[rgb(var(--color-foreground))] mb-4 transition-colors"
          @click="router.push(`/courses/${courseId}`)"
        >
          &larr; Back to course
        </button>
        <h2 class="text-sm font-semibold mb-3">{{ course.title }}</h2>

        <div v-for="ch in chapters" :key="ch.id" class="mb-3">
          <div class="text-xs font-medium text-[rgb(var(--color-muted-foreground))] mb-1 uppercase tracking-wider">
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
              class="w-4 h-4 rounded-full border flex items-center justify-center shrink-0"
              :class="elementStatus(el.id) === 'completed'
                ? 'bg-[rgb(var(--color-success))] border-[rgb(var(--color-success))] text-white'
                : 'border-[rgb(var(--color-border))]'"
            >
              <svg v-if="elementStatus(el.id) === 'completed'" class="w-2.5 h-2.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="3">
                <path stroke-linecap="round" stroke-linejoin="round" d="M5 13l4 4L19 7" />
              </svg>
            </span>
            <span class="truncate">{{ el.title }}</span>
          </button>
        </div>
      </div>

      <!-- Main content area -->
      <div class="flex-1 overflow-y-auto">
        <div v-if="currentElement" class="max-w-3xl">
          <div class="flex items-center gap-2 mb-2">
            <StatusBadge :status="currentElement.element_type" />
            <span v-if="currentElement.duration_seconds" class="text-xs text-[rgb(var(--color-muted-foreground))]">
              {{ Math.round(currentElement.duration_seconds / 60) }} min
            </span>
          </div>
          <h1 class="text-lg font-bold mb-4">{{ currentElement.title }}</h1>

          <!-- Content placeholder -->
          <div class="card p-8 text-center mb-6">
            <div v-if="currentElement.content_cid" class="text-sm text-[rgb(var(--color-muted-foreground))]">
              <p class="mb-2">Content CID:</p>
              <code class="font-mono text-xs break-all">{{ currentElement.content_cid }}</code>
              <p class="mt-4 text-xs">Content rendering (video/PDF/quiz) will be implemented with IPFS content resolution.</p>
            </div>
            <div v-else class="text-sm text-[rgb(var(--color-muted-foreground))]">
              No content attached to this element yet.
            </div>
          </div>

          <!-- Actions -->
          <div v-if="enrollment" class="flex gap-2">
            <AppButton
              v-if="elementStatus(currentElement.id) !== 'completed'"
              @click="markComplete"
            >
              Mark as Complete
            </AppButton>
            <div v-else class="flex items-center gap-2 text-sm text-[rgb(var(--color-success))]">
              <svg class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                <path stroke-linecap="round" stroke-linejoin="round" d="M5 13l4 4L19 7" />
              </svg>
              Completed
            </div>
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
