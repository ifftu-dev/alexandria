<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useLocalApi } from '@/composables/useLocalApi'
import { AppButton } from '@/components/ui'
import CourseCard from '@/components/course/CourseCard.vue'
import type { Course } from '@/types'

const { invoke } = useLocalApi()

const courses = ref<Course[]>([])
const loading = ref(true)

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
    <div class="flex items-center justify-between mb-6">
      <div>
        <h1 class="text-xl font-bold">Courses</h1>
        <p class="text-sm text-muted-foreground">
          Courses on your node and discovered from peers.
        </p>
      </div>
      <AppButton variant="primary" size="sm" @click="$router.push('/instructor/courses/new')">
        <svg class="w-4 h-4 mr-1" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
          <path stroke-linecap="round" stroke-linejoin="round" d="M12 4v16m8-8H4" />
        </svg>
        Create Course
      </AppButton>
    </div>

    <!-- Loading skeleton (Mark 2 style: shadow only, no border) -->
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

    <!-- Empty state (Mark 2 style: shadow, no border) -->
    <div
      v-else-if="courses.length === 0"
      class="rounded-xl bg-card p-16 text-center shadow-sm"
    >
      <div class="mx-auto mb-4 flex h-14 w-14 items-center justify-center rounded-full bg-primary/10">
        <svg class="h-7 w-7 text-primary" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
          <path stroke-linecap="round" stroke-linejoin="round" d="M12 6.253v13m0-13C10.832 5.477 9.246 5 7.5 5S4.168 5.477 3 6.253v13C4.168 18.477 5.754 18 7.5 18s3.332.477 4.5 1.253m0-13C13.168 5.477 14.754 5 16.5 5c1.747 0 3.332.477 4.5 1.253v13C19.832 18.477 18.247 18 16.5 18c-1.746 0-3.332.477-4.5 1.253" />
        </svg>
      </div>
      <p class="text-sm font-medium text-foreground">No courses yet</p>
      <p class="mt-1 text-xs text-muted-foreground max-w-sm mx-auto">
        Courses will appear here when you create them locally or discover them from peers.
      </p>
      <AppButton variant="primary" size="sm" class="mt-5" @click="$router.push('/instructor/courses/new')">
        Create Your First Course
      </AppButton>
    </div>

    <!-- Course grid (Mark 2 style: gap-6, 4 columns max) -->
    <div v-else class="grid gap-6 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4">
      <CourseCard
        v-for="course in courses"
        :key="course.id"
        :course="course"
      />
    </div>
  </div>
</template>
