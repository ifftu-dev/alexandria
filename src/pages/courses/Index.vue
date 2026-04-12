<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useLocalApi } from '@/composables/useLocalApi'
import { AppButton } from '@/components/ui'
import CourseCard from '@/components/course/CourseCard.vue'
import type { Course } from '@/types'

const { invoke } = useLocalApi()

const courses = ref<Course[]>([])
const loading = ref(true)
// Kind filter: 'all' | 'course' | 'tutorial'. Persisted in sessionStorage
// so navigating away and back doesn't reset the user's selection.
const kindFilter = ref<'all' | 'course' | 'tutorial'>(
  (sessionStorage.getItem('courses-kind-filter') as 'all' | 'course' | 'tutorial') || 'all',
)

function setKindFilter(k: 'all' | 'course' | 'tutorial') {
  kindFilter.value = k
  sessionStorage.setItem('courses-kind-filter', k)
}

const filteredCourses = computed(() =>
  kindFilter.value === 'all'
    ? courses.value
    : courses.value.filter((c) => c.kind === kindFilter.value),
)

const counts = computed(() => ({
  all: courses.value.length,
  course: courses.value.filter((c) => c.kind === 'course').length,
  tutorial: courses.value.filter((c) => c.kind === 'tutorial').length,
}))

onMounted(async () => {
  try {
    await invoke<number>('bootstrap_public_catalog').catch((e) => {
      console.warn('Public catalog bootstrap skipped:', e)
      return 0
    })
    await invoke<number>('hydrate_catalog_courses', { limit: 200 }).catch((e) => {
      console.warn('Catalog hydration skipped:', e)
      return 0
    })
    courses.value = await invoke<Course[]>('list_courses')
  } catch (e) {
    console.error('Failed to load courses:', e)
  } finally {
    loading.value = false
  }
})
</script>

<template>
  <div>
    <div class="flex items-center justify-between mb-4">
      <div>
        <h1 class="text-xl font-bold">Courses</h1>
        <p class="text-sm text-muted-foreground">
          Courses and tutorials on your node, and those discovered from peers.
        </p>
      </div>
      <div class="flex gap-2">
        <AppButton variant="ghost" size="sm" @click="$router.push('/instructor/tutorials/new')">
          <svg class="w-4 h-4 mr-1" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
            <path stroke-linecap="round" stroke-linejoin="round" d="M8 5v14l11-7z" />
          </svg>
          New Tutorial
        </AppButton>
        <AppButton variant="primary" size="sm" @click="$router.push('/instructor/courses/new')">
          <svg class="w-4 h-4 mr-1" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
            <path stroke-linecap="round" stroke-linejoin="round" d="M12 4v16m8-8H4" />
          </svg>
          New Course
        </AppButton>
      </div>
    </div>

    <!-- Kind filter chips -->
    <div class="flex items-center gap-2 mb-6">
      <button
        v-for="opt in (['all', 'course', 'tutorial'] as const)"
        :key="opt"
        type="button"
        :class="[
          'text-xs font-medium px-3 py-1.5 rounded-full transition-colors',
          kindFilter === opt
            ? 'bg-primary text-primary-foreground'
            : 'bg-muted text-muted-foreground hover:bg-muted/70',
        ]"
        @click="setKindFilter(opt)"
      >
        {{ opt === 'all' ? 'All' : opt === 'course' ? 'Courses' : 'Tutorials' }}
        <span class="ml-1 opacity-70">{{ counts[opt] }}</span>
      </button>
    </div>

    <!-- Loading skeleton (shadow only, no border) -->
    <div v-if="loading" class="grid gap-6 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4">
      <div v-for="i in 8" :key="i" class="animate-pulse overflow-hidden rounded-xl bg-card shadow-sm">
        <div class="aspect-[16/9] bg-muted" />
        <div class="p-4 space-y-2">
          <div class="flex gap-2">
            <div class="h-4 w-16 rounded bg-muted" />
            <div class="h-4 w-12 rounded bg-muted" />
          </div>
          <div class="h-5 w-4/5 rounded bg-muted" />
          <div class="h-4 w-full rounded bg-muted" />
          <div class="flex gap-2 pt-2">
            <div class="h-5 w-5 rounded-full bg-muted" />
            <div class="h-3 w-20 rounded bg-muted" />
          </div>
        </div>
      </div>
    </div>

    <!-- Empty state (shadow, no border) -->
    <div
      v-else-if="filteredCourses.length === 0"
      class="rounded-xl bg-card p-16 text-center shadow-sm"
    >
      <div class="mx-auto mb-4 flex h-14 w-14 items-center justify-center rounded-full bg-primary/10">
        <svg class="h-7 w-7 text-primary" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
          <path stroke-linecap="round" stroke-linejoin="round" d="M12 6.253v13m0-13C10.832 5.477 9.246 5 7.5 5S4.168 5.477 3 6.253v13C4.168 18.477 5.754 18 7.5 18s3.332.477 4.5 1.253m0-13C13.168 5.477 14.754 5 16.5 5c1.747 0 3.332.477 4.5 1.253v13C19.832 18.477 18.247 18 16.5 18c-1.746 0-3.332.477-4.5 1.253" />
        </svg>
      </div>
      <p class="text-sm font-medium text-foreground">
        {{ kindFilter === 'tutorial' ? 'No tutorials yet' :
           kindFilter === 'course' ? 'No courses yet' : 'No courses yet' }}
      </p>
      <p class="mt-1 text-xs text-muted-foreground max-w-sm mx-auto">
        <template v-if="kindFilter === 'tutorial'">
          Tutorials are single-video learning artefacts. Create one to share a
          focused explainer with skill tags.
        </template>
        <template v-else>
          Courses will appear here when you create them locally or discover them from peers.
        </template>
      </p>
      <AppButton
        variant="primary"
        size="sm"
        class="mt-5"
        @click="$router.push(
          kindFilter === 'tutorial'
            ? '/instructor/tutorials/new'
            : '/instructor/courses/new'
        )"
      >
        {{ kindFilter === 'tutorial' ? 'Create Your First Tutorial' : 'Create Your First Course' }}
      </AppButton>
    </div>

    <!-- Course grid (gap-6, 4 columns max) -->
    <div v-else class="grid gap-6 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4">
      <CourseCard
        v-for="course in filteredCourses"
        :key="course.id"
        :course="course"
      />
    </div>
  </div>
</template>
