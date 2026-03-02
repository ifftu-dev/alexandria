<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useLocalApi } from '@/composables/useLocalApi'
import { AppButton, StatusBadge } from '@/components/ui'
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

    <!-- Loading skeleton -->
    <div v-if="loading" class="grid gap-5 sm:grid-cols-2 lg:grid-cols-3">
      <div v-for="i in 6" :key="i" class="animate-pulse overflow-hidden rounded-xl bg-card border border-border shadow-sm">
        <div class="aspect-[16/9] bg-muted" />
        <div class="p-4 space-y-2">
          <div class="h-4 w-3/4 rounded bg-muted" />
          <div class="h-3 w-full rounded bg-muted" />
          <div class="h-3 w-2/3 rounded bg-muted" />
          <div class="flex gap-2 pt-2">
            <div class="h-5 w-16 rounded-full bg-muted" />
            <div class="h-5 w-10 rounded-full bg-muted" />
          </div>
        </div>
      </div>
    </div>

    <!-- Empty state -->
    <div
      v-else-if="courses.length === 0"
      class="rounded-xl bg-card border border-border p-16 text-center"
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

    <!-- Course grid -->
    <div v-else class="grid gap-5 sm:grid-cols-2 lg:grid-cols-3">
      <router-link
        v-for="course in courses"
        :key="course.id"
        :to="`/courses/${course.id}`"
        class="group"
      >
        <div class="card card-interactive overflow-hidden h-full flex flex-col">
          <!-- Thumbnail -->
          <div class="aspect-[16/9] relative overflow-hidden">
            <div v-if="course.thumbnail_svg" class="w-full h-full" v-html="course.thumbnail_svg" />
            <div v-else class="w-full h-full bg-gradient-to-br from-primary/10 to-accent/5 flex items-center justify-center">
              <svg class="w-8 h-8 text-primary/30" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
                <path stroke-linecap="round" stroke-linejoin="round" d="M12 6.253v13m0-13C10.832 5.477 9.246 5 7.5 5S4.168 5.477 3 6.253v13C4.168 18.477 5.754 18 7.5 18s3.332.477 4.5 1.253m0-13C13.168 5.477 14.754 5 16.5 5c1.747 0 3.332.477 4.5 1.253v13C19.832 18.477 18.247 18 16.5 18c-1.746 0-3.332.477-4.5 1.253" />
              </svg>
            </div>
            <!-- Status badge overlay -->
            <div class="absolute top-2.5 right-2.5">
              <StatusBadge :status="course.status" />
            </div>
          </div>

          <div class="p-4 flex-1 flex flex-col">
            <!-- Tags row -->
            <div v-if="course.tags?.length" class="flex flex-wrap gap-1.5 mb-2">
              <span
                v-for="tag in course.tags.slice(0, 3)"
                :key="tag"
                class="text-[10px] font-medium px-1.5 py-0.5 rounded bg-muted text-muted-foreground"
              >
                {{ tag }}
              </span>
              <span v-if="course.tags.length > 3" class="text-[10px] text-muted-foreground">
                +{{ course.tags.length - 3 }}
              </span>
            </div>

            <!-- Title -->
            <h3 class="text-sm font-semibold text-foreground line-clamp-2 group-hover:text-primary transition-colors">
              {{ course.title }}
            </h3>

            <!-- Description -->
            <p v-if="course.description" class="text-xs text-muted-foreground line-clamp-2 mt-1.5 flex-1">
              {{ course.description }}
            </p>

            <!-- Footer -->
            <div class="flex items-center gap-2 mt-3 pt-3 border-t border-border/50">
              <!-- Author avatar -->
              <div class="w-5 h-5 rounded-full bg-gradient-to-br from-primary to-accent flex items-center justify-center shrink-0">
                <span class="text-[8px] font-bold text-white">{{ (course.author_name || 'A').charAt(0) }}</span>
              </div>
              <span class="text-[10px] text-muted-foreground truncate flex-1">
                {{ course.author_name || (course.author_address ? course.author_address.slice(0, 16) + '...' : 'Unknown') }}
              </span>
              <span class="text-[10px] text-muted-foreground">v{{ course.version }}</span>
            </div>
          </div>
        </div>
      </router-link>
    </div>
  </div>
</template>
