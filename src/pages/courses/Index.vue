<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useLocalApi } from '@/composables/useLocalApi'

const { invoke } = useLocalApi()

interface Course {
  id: string
  title: string
  description: string | null
  author_address: string
  status: string
  tags: string[] | null
  created_at: string
}

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
          Courses available on your node. In Phase 3, this catalog will be populated via P2P gossip.
        </p>
      </div>
    </div>

    <div v-if="loading" class="text-sm text-[rgb(var(--color-muted-foreground))]">Loading...</div>

    <div v-else-if="courses.length === 0" class="card p-8 text-center">
      <p class="text-sm text-[rgb(var(--color-muted-foreground))] mb-2">No courses yet.</p>
      <p class="text-xs text-[rgb(var(--color-muted-foreground))]">
        Courses will appear here when you create them locally or discover them from peers.
      </p>
    </div>

    <div v-else class="grid grid-cols-2 gap-4">
      <router-link
        v-for="course in courses"
        :key="course.id"
        :to="`/courses/${course.id}`"
        class="card p-4 hover:shadow-md transition-shadow"
      >
        <div class="text-sm font-medium mb-1">{{ course.title }}</div>
        <p v-if="course.description" class="text-xs text-[rgb(var(--color-muted-foreground))] line-clamp-2 mb-2">
          {{ course.description }}
        </p>
        <div class="flex items-center gap-2 text-xs text-[rgb(var(--color-muted-foreground))]">
          <span class="px-1.5 py-0.5 rounded bg-[rgb(var(--color-muted)/0.5)]">{{ course.status }}</span>
          <span v-if="course.tags?.length">{{ course.tags.join(', ') }}</span>
        </div>
      </router-link>
    </div>
  </div>
</template>
