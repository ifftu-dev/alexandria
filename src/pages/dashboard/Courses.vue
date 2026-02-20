<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useLocalApi } from '@/composables/useLocalApi'

const { invoke } = useLocalApi()

interface Enrollment {
  id: string
  course_id: string
  enrolled_at: string
  completed_at: string | null
  status: string
}

const enrollments = ref<Enrollment[]>([])
const loading = ref(true)

onMounted(async () => {
  try {
    enrollments.value = await invoke<Enrollment[]>('list_enrollments')
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

    <div v-if="loading" class="text-sm text-[rgb(var(--color-muted-foreground))]">Loading...</div>

    <div v-else-if="enrollments.length === 0" class="card p-8 text-center">
      <p class="text-sm text-[rgb(var(--color-muted-foreground))]">
        No enrollments yet. Browse courses and enroll to start learning.
      </p>
    </div>

    <div v-else class="space-y-3">
      <div
        v-for="enrollment in enrollments"
        :key="enrollment.id"
        class="card p-4 flex items-center justify-between"
      >
        <div>
          <div class="text-sm font-medium">{{ enrollment.course_id }}</div>
          <div class="text-xs text-[rgb(var(--color-muted-foreground))]">
            Enrolled {{ enrollment.enrolled_at }}
          </div>
        </div>
        <span
          class="px-2 py-0.5 rounded text-xs"
          :class="enrollment.status === 'completed'
            ? 'bg-[rgb(var(--color-success)/0.1)] text-[rgb(var(--color-success))]'
            : 'bg-[rgb(var(--color-primary)/0.1)] text-[rgb(var(--color-primary))]'"
        >
          {{ enrollment.status }}
        </span>
      </div>
    </div>
  </div>
</template>
