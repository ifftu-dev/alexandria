<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useLocalApi } from '@/composables/useLocalApi'
import { AppSpinner, AppButton, EmptyState, StatusBadge } from '@/components/ui'
import type { Enrollment, Course } from '@/types'

const { invoke } = useLocalApi()

const enrollments = ref<Enrollment[]>([])
const courseMap = ref<Record<string, Course>>({})
const loading = ref(true)

onMounted(async () => {
  try {
    enrollments.value = await invoke<Enrollment[]>('list_enrollments')

    // Fetch course details for enrolled courses
    const courses = await invoke<Course[]>('list_courses')
    for (const c of courses) {
      courseMap.value[c.id] = c
    }
  } catch (e) {
    console.error('Failed to load enrollments:', e)
  } finally {
    loading.value = false
  }
})
</script>

<template>
  <div>
    <h1 class="text-xl font-bold mb-1">My Courses</h1>
    <p class="text-sm text-[rgb(var(--color-muted-foreground))] mb-6">
      Courses you're enrolled in.
    </p>

    <AppSpinner v-if="loading" label="Loading enrollments..." />

    <EmptyState
      v-else-if="enrollments.length === 0"
      title="No enrollments yet"
      description="Browse courses and enroll to start learning."
    >
      <template #action>
        <AppButton variant="outline" size="sm" @click="$router.push('/courses')">
          Browse Courses
        </AppButton>
      </template>
    </EmptyState>

    <div v-else class="space-y-3">
      <router-link
        v-for="enrollment in enrollments"
        :key="enrollment.id"
        :to="enrollment.status === 'active' ? `/learn/${enrollment.course_id}` : `/courses/${enrollment.course_id}`"
        class="card card-interactive p-4 flex items-center justify-between"
      >
        <div>
          <div class="text-sm font-medium">
            {{ courseMap[enrollment.course_id]?.title ?? enrollment.course_id }}
          </div>
          <div class="text-xs text-[rgb(var(--color-muted-foreground))]">
            Enrolled {{ enrollment.enrolled_at }}
          </div>
        </div>
        <StatusBadge :status="enrollment.status" />
      </router-link>
    </div>
  </div>
</template>
