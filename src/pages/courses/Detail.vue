<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useRoute } from 'vue-router'
import { useLocalApi } from '@/composables/useLocalApi'

const { invoke } = useLocalApi()
const route = useRoute()

interface Course {
  id: string
  title: string
  description: string | null
  author_address: string
  content_cid: string | null
  status: string
  tags: string[] | null
  skill_ids: string[] | null
  version: number
  created_at: string
  updated_at: string
}

const course = ref<Course | null>(null)
const loading = ref(true)

onMounted(async () => {
  try {
    course.value = await invoke<Course | null>('get_course', {
      courseId: route.params.id as string,
    })
  } catch (e) {
    console.error('Failed to load course:', e)
  } finally {
    loading.value = false
  }
})
</script>

<template>
  <div>
    <div v-if="loading" class="text-sm text-[rgb(var(--color-muted-foreground))]">Loading...</div>

    <div v-else-if="!course" class="card p-8 text-center">
      <p class="text-sm text-[rgb(var(--color-muted-foreground))]">Course not found.</p>
    </div>

    <div v-else>
      <div class="flex items-center gap-2 mb-1">
        <span class="px-1.5 py-0.5 rounded text-xs bg-[rgb(var(--color-muted)/0.5)] text-[rgb(var(--color-muted-foreground))]">
          {{ course.status }}
        </span>
        <span class="text-xs text-[rgb(var(--color-muted-foreground))]">v{{ course.version }}</span>
      </div>

      <h1 class="text-xl font-bold mb-2">{{ course.title }}</h1>
      <p v-if="course.description" class="text-sm text-[rgb(var(--color-muted-foreground))] mb-6">
        {{ course.description }}
      </p>

      <div class="card p-5">
        <h2 class="text-base font-semibold mb-3">Details</h2>
        <div class="space-y-2 text-sm">
          <div class="flex gap-2">
            <span class="text-[rgb(var(--color-muted-foreground))] w-24">Author</span>
            <code class="font-mono text-xs break-all">{{ course.author_address }}</code>
          </div>
          <div v-if="course.content_cid" class="flex gap-2">
            <span class="text-[rgb(var(--color-muted-foreground))] w-24">Content CID</span>
            <code class="font-mono text-xs break-all">{{ course.content_cid }}</code>
          </div>
          <div v-if="course.tags?.length" class="flex gap-2">
            <span class="text-[rgb(var(--color-muted-foreground))] w-24">Tags</span>
            <span>{{ course.tags.join(', ') }}</span>
          </div>
          <div class="flex gap-2">
            <span class="text-[rgb(var(--color-muted-foreground))] w-24">Created</span>
            <span>{{ course.created_at }}</span>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>
