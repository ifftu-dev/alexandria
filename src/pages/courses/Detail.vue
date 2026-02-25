<script setup lang="ts">
import { ref, onMounted, computed } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useLocalApi } from '@/composables/useLocalApi'
import { AppButton, StatusBadge, EmptyState } from '@/components/ui'
import type { Course, Chapter, Element, Enrollment } from '@/types'

const { invoke } = useLocalApi()
const route = useRoute()
const router = useRouter()

const course = ref<Course | null>(null)
const chapters = ref<Chapter[]>([])
const elements = ref<Record<string, Element[]>>({})
const enrollment = ref<Enrollment | null>(null)
const loading = ref(true)
const enrolling = ref(false)

const totalElements = computed(() => {
  let count = 0
  for (const elems of Object.values(elements.value)) {
    count += elems.length
  }
  return count
})

const elementTypeCounts = computed(() => {
  const counts: Record<string, number> = {}
  for (const elems of Object.values(elements.value)) {
    for (const el of elems) {
      counts[el.element_type] = (counts[el.element_type] || 0) + 1
    }
  }
  return counts
})

onMounted(async () => {
  const courseId = route.params.id as string
  try {
    const [c, chs, enrollments] = await Promise.all([
      invoke<Course | null>('get_course', { courseId }),
      invoke<Chapter[]>('list_chapters', { courseId }).catch(() => []),
      invoke<Enrollment[]>('list_enrollments').catch(() => []),
    ])
    course.value = c
    chapters.value = chs
    enrollment.value = enrollments.find(e => e.course_id === courseId) ?? null

    // Load elements for each chapter
    for (const ch of chs) {
      elements.value[ch.id] = await invoke<Element[]>('list_elements', { chapterId: ch.id }).catch(() => [])
    }
  } catch (e) {
    console.error('Failed to load course:', e)
  } finally {
    loading.value = false
  }
})

async function enroll() {
  if (!course.value) return
  enrolling.value = true
  try {
    enrollment.value = await invoke<Enrollment>('enroll', { courseId: course.value.id })
  } catch (e) {
    console.error('Failed to enroll:', e)
  } finally {
    enrolling.value = false
  }
}

function elementTypeIcon(elementType: string): string {
  switch (elementType) {
    case 'video': return 'M14.752 11.168l-3.197-2.132A1 1 0 0010 9.87v4.263a1 1 0 001.555.832l3.197-2.132a1 1 0 000-1.664z'
    case 'text': return 'M4 6h16M4 12h16M4 18h7'
    case 'pdf': return 'M9 12h6m-6 4h6m2 5H7a2 2 0 01-2-2V5a2 2 0 012-2h5.586a1 1 0 01.707.293l5.414 5.414a1 1 0 01.293.707V19a2 2 0 01-2 2z'
    case 'downloadable': return 'M4 16v1a3 3 0 003 3h10a3 3 0 003-3v-1m-4-4l-4 4m0 0l-4-4m4 4V4'
    case 'quiz': case 'assessment': case 'objective_single_mcq': case 'objective_multi_mcq': case 'subjective_mcq':
      return 'M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z'
    case 'essay': return 'M11 5H6a2 2 0 00-2 2v11a2 2 0 002 2h11a2 2 0 002-2v-5m-1.414-9.414a2 2 0 112.828 2.828L11.828 15H9v-2.828l8.586-8.586z'
    case 'interactive': return 'M13 10V3L4 14h7v7l9-11h-7z'
    default: return 'M7 21h10a2 2 0 002-2V9.414a1 1 0 00-.293-.707l-5.414-5.414A1 1 0 0012.586 3H7a2 2 0 00-2 2v14a2 2 0 002 2z'
  }
}

function elementTypeLabel(elementType: string): string {
  switch (elementType) {
    case 'video': return 'Video'
    case 'text': return 'Reading'
    case 'pdf': return 'PDF'
    case 'downloadable': return 'Download'
    case 'quiz': return 'Quiz'
    case 'assessment': return 'Assessment'
    case 'objective_single_mcq': return 'Single Choice'
    case 'objective_multi_mcq': return 'Multiple Choice'
    case 'subjective_mcq': return 'Subjective'
    case 'essay': return 'Essay'
    case 'interactive': return 'Interactive'
    default: return elementType
  }
}
</script>

<template>
  <div>
    <!-- Loading skeleton -->
    <div v-if="loading" class="animate-pulse space-y-6">
      <div class="flex items-start justify-between">
        <div class="space-y-2">
          <div class="flex gap-2">
            <div class="h-5 w-16 rounded-full bg-[rgb(var(--color-muted))]" />
            <div class="h-5 w-10 rounded bg-[rgb(var(--color-muted))]" />
          </div>
          <div class="h-7 w-80 rounded bg-[rgb(var(--color-muted))]" />
          <div class="h-4 w-96 rounded bg-[rgb(var(--color-muted))]" />
        </div>
        <div class="h-10 w-28 rounded-lg bg-[rgb(var(--color-muted))]" />
      </div>
      <div class="card p-5 space-y-3">
        <div class="h-5 w-24 rounded bg-[rgb(var(--color-muted))]" />
        <div v-for="i in 3" :key="i" class="h-16 rounded-lg bg-[rgb(var(--color-muted)/0.3)]" />
      </div>
    </div>

    <EmptyState
      v-else-if="!course"
      title="Course not found"
      description="This course may have been removed or is not available on your node."
    />

    <div v-else class="max-w-4xl">
      <!-- Header -->
      <div class="flex items-start justify-between gap-6 mb-8">
        <div class="min-w-0">
          <div class="flex items-center gap-2 mb-2">
            <StatusBadge :status="course.status" />
            <span class="text-xs text-[rgb(var(--color-muted-foreground))]">v{{ course.version }}</span>
          </div>
          <h1 class="text-2xl font-bold tracking-tight">{{ course.title }}</h1>
          <p v-if="course.description" class="text-sm text-[rgb(var(--color-muted-foreground))] mt-2 max-w-2xl">
            {{ course.description }}
          </p>

          <!-- Stats pills -->
          <div class="flex items-center gap-3 mt-4">
            <span class="inline-flex items-center gap-1.5 text-xs text-[rgb(var(--color-muted-foreground))]">
              <svg class="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                <path stroke-linecap="round" stroke-linejoin="round" d="M19 11H5m14 0a2 2 0 012 2v6a2 2 0 01-2 2H5a2 2 0 01-2-2v-6a2 2 0 012-2m14 0V9a2 2 0 00-2-2M5 11V9a2 2 0 012-2m0 0V5a2 2 0 012-2h6a2 2 0 012 2v2M7 7h10" />
              </svg>
              {{ chapters.length }} chapter{{ chapters.length !== 1 ? 's' : '' }}
            </span>
            <span class="inline-flex items-center gap-1.5 text-xs text-[rgb(var(--color-muted-foreground))]">
              <svg class="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                <path stroke-linecap="round" stroke-linejoin="round" d="M9 12h6m-6 4h6m2 5H7a2 2 0 01-2-2V5a2 2 0 012-2h5.586a1 1 0 01.707.293l5.414 5.414a1 1 0 01.293.707V19a2 2 0 01-2 2z" />
              </svg>
              {{ totalElements }} element{{ totalElements !== 1 ? 's' : '' }}
            </span>
            <span v-if="course.tags?.length" class="inline-flex items-center gap-1.5 text-xs text-[rgb(var(--color-muted-foreground))]">
              <svg class="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                <path stroke-linecap="round" stroke-linejoin="round" d="M7 7h.01M7 3h5c.512 0 1.024.195 1.414.586l7 7a2 2 0 010 2.828l-7 7a2 2 0 01-2.828 0l-7-7A1.994 1.994 0 013 12V7a4 4 0 014-4z" />
              </svg>
              {{ course.tags.length }} tag{{ course.tags.length !== 1 ? 's' : '' }}
            </span>
          </div>
        </div>

        <div class="shrink-0 flex flex-col gap-2">
          <AppButton
            v-if="!enrollment"
            :loading="enrolling"
            variant="primary"
            @click="enroll"
          >
            <svg class="w-4 h-4 mr-1" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
              <path stroke-linecap="round" stroke-linejoin="round" d="M12 4v16m8-8H4" />
            </svg>
            Enroll
          </AppButton>
          <AppButton
            v-else-if="enrollment.status === 'active'"
            variant="primary"
            @click="router.push(`/learn/${course.id}`)"
          >
            <svg class="w-4 h-4 mr-1" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
              <path stroke-linecap="round" stroke-linejoin="round" d="M14.752 11.168l-3.197-2.132A1 1 0 0010 9.87v4.263a1 1 0 001.555.832l3.197-2.132a1 1 0 000-1.664z" />
            </svg>
            Continue Learning
          </AppButton>
          <div v-else class="flex justify-end">
            <StatusBadge :status="enrollment.status" />
          </div>
          <AppButton
            variant="ghost"
            size="sm"
            @click="router.push(`/instructor/courses/${course.id}`)"
          >
            Edit Course
          </AppButton>
        </div>
      </div>

      <!-- Tags -->
      <div v-if="course.tags?.length" class="flex flex-wrap gap-2 mb-6">
        <span
          v-for="tag in course.tags"
          :key="tag"
          class="badge badge-secondary"
        >
          {{ tag }}
        </span>
      </div>

      <!-- Element type breakdown -->
      <div v-if="Object.keys(elementTypeCounts).length > 0" class="flex flex-wrap gap-2 mb-6">
        <span
          v-for="(count, type) in elementTypeCounts"
          :key="type"
          class="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-lg bg-[rgb(var(--color-muted)/0.5)] text-xs text-[rgb(var(--color-muted-foreground))]"
        >
          <svg class="w-3 h-3" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
            <path stroke-linecap="round" stroke-linejoin="round" :d="elementTypeIcon(type as string)" />
          </svg>
          {{ count }} {{ elementTypeLabel(type as string) }}
        </span>
      </div>

      <!-- Chapters with elements -->
      <div v-if="chapters.length > 0" class="space-y-4 mb-8">
        <h2 class="text-base font-semibold">Course Content</h2>
        <div
          v-for="(chapter, index) in chapters"
          :key="chapter.id"
          class="card overflow-hidden"
        >
          <!-- Chapter header -->
          <div class="flex items-center gap-3 px-5 py-4 bg-[rgb(var(--color-muted)/0.2)]">
            <span class="flex h-7 w-7 items-center justify-center rounded-full bg-[rgb(var(--color-primary)/0.1)] text-xs font-semibold text-[rgb(var(--color-primary))]">
              {{ index + 1 }}
            </span>
            <div class="min-w-0">
              <h3 class="text-sm font-semibold truncate">{{ chapter.title }}</h3>
              <p v-if="chapter.description" class="text-xs text-[rgb(var(--color-muted-foreground))] truncate">
                {{ chapter.description }}
              </p>
            </div>
            <span v-if="elements[chapter.id]?.length" class="ml-auto text-xs text-[rgb(var(--color-muted-foreground))] shrink-0">
              {{ elements[chapter.id]?.length ?? 0 }} element{{ (elements[chapter.id]?.length ?? 0) !== 1 ? 's' : '' }}
            </span>
          </div>

          <!-- Elements list -->
          <div v-if="elements[chapter.id]?.length" class="divide-y divide-[rgb(var(--color-border)/0.5)]">
            <div
              v-for="el in elements[chapter.id]"
              :key="el.id"
              class="flex items-center gap-3 px-5 py-3"
            >
              <span class="flex h-6 w-6 items-center justify-center rounded bg-[rgb(var(--color-muted))] shrink-0">
                <svg class="w-3.5 h-3.5 text-[rgb(var(--color-muted-foreground))]" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                  <path stroke-linecap="round" stroke-linejoin="round" :d="elementTypeIcon(el.element_type)" />
                </svg>
              </span>
              <span class="text-sm truncate">{{ el.title }}</span>
              <span class="ml-auto badge badge-secondary text-[10px]">
                {{ elementTypeLabel(el.element_type) }}
              </span>
            </div>
          </div>
          <div v-else class="px-5 py-3 text-xs text-[rgb(var(--color-muted-foreground))] italic">
            No elements yet
          </div>
        </div>
      </div>

      <!-- Details card -->
      <div class="card p-5">
        <h2 class="text-base font-semibold mb-4">Details</h2>
        <div class="grid grid-cols-1 sm:grid-cols-2 gap-4">
          <div>
            <div class="text-xs text-[rgb(var(--color-muted-foreground))] mb-0.5">Author</div>
            <div class="text-sm text-[rgb(var(--color-foreground))]">
              <span v-if="course.author_name" class="font-medium">{{ course.author_name }}</span>
              <span v-else class="font-mono break-all">{{ course.author_address || 'Unknown' }}</span>
            </div>
            <div v-if="course.author_name && course.author_address" class="text-xs font-mono text-[rgb(var(--color-muted-foreground))] mt-0.5 break-all">
              {{ course.author_address }}
            </div>
          </div>
          <div v-if="course.content_cid">
            <div class="text-xs text-[rgb(var(--color-muted-foreground))] mb-0.5">Content CID</div>
            <div class="text-sm font-mono text-[rgb(var(--color-foreground))] break-all">
              {{ course.content_cid }}
            </div>
          </div>
          <div v-if="course.skill_ids?.length">
            <div class="text-xs text-[rgb(var(--color-muted-foreground))] mb-0.5">Linked Skills</div>
            <div class="text-sm text-[rgb(var(--color-foreground))]">
              {{ course.skill_ids.length }} skill{{ course.skill_ids.length !== 1 ? 's' : '' }}
            </div>
          </div>
          <div>
            <div class="text-xs text-[rgb(var(--color-muted-foreground))] mb-0.5">Created</div>
            <div class="text-sm text-[rgb(var(--color-foreground))]">
              {{ new Date(course.created_at).toLocaleDateString() }}
            </div>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>
