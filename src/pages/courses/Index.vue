<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useLocalApi } from '@/composables/useLocalApi'
import { AppSpinner, AppButton, EmptyState, StatusBadge } from '@/components/ui'
import type { Course } from '@/types'

const { invoke } = useLocalApi()

const courses = ref<Course[]>([])
const loading = ref(true)

onMounted(async () => {
  try {
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
        <p class="text-sm text-[rgb(var(--color-muted-foreground))]">
          Courses on your node and discovered from peers.
        </p>
      </div>
      <AppButton variant="primary" size="sm" @click="$router.push('/instructor/courses/new')">
        Create Course
      </AppButton>
    </div>

    <AppSpinner v-if="loading" label="Loading courses..." />

    <EmptyState
      v-else-if="courses.length === 0"
      title="No courses yet"
      description="Courses will appear here when you create them locally or discover them from peers."
    >
      <template #action>
        <AppButton variant="outline" size="sm" @click="$router.push('/instructor/courses/new')">
          Create Your First Course
        </AppButton>
      </template>
    </EmptyState>

    <div v-else class="grid grid-cols-1 sm:grid-cols-2 gap-4">
      <router-link
        v-for="course in courses"
        :key="course.id"
        :to="`/courses/${course.id}`"
        class="card card-interactive p-4"
      >
        <div class="text-sm font-medium mb-1">{{ course.title }}</div>
        <p v-if="course.description" class="text-xs text-[rgb(var(--color-muted-foreground))] line-clamp-2 mb-2">
          {{ course.description }}
        </p>
        <div class="flex items-center gap-2">
          <StatusBadge :status="course.status" />
          <span v-if="course.tags?.length" class="text-xs text-[rgb(var(--color-muted-foreground))]">
            {{ course.tags.join(', ') }}
          </span>
        </div>
      </router-link>
    </div>
  </div>
</template>
