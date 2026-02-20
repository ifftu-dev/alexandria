<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useLocalApi } from '@/composables/useLocalApi'
import { AppSpinner, AppButton, StatusBadge, DataRow, EmptyState } from '@/components/ui'
import type { Course, Chapter, Enrollment } from '@/types'

const { invoke } = useLocalApi()
const route = useRoute()
const router = useRouter()

const course = ref<Course | null>(null)
const chapters = ref<Chapter[]>([])
const enrollment = ref<Enrollment | null>(null)
const loading = ref(true)
const enrolling = ref(false)

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
</script>

<template>
  <div>
    <AppSpinner v-if="loading" label="Loading course..." />

    <EmptyState
      v-else-if="!course"
      title="Course not found"
      description="This course may have been removed or is not available on your node."
    />

    <div v-else>
      <!-- Header -->
      <div class="flex items-start justify-between mb-6">
        <div>
          <div class="flex items-center gap-2 mb-1">
            <StatusBadge :status="course.status" />
            <span class="text-xs text-[rgb(var(--color-muted-foreground))]">v{{ course.version }}</span>
          </div>
          <h1 class="text-xl font-bold">{{ course.title }}</h1>
          <p v-if="course.description" class="text-sm text-[rgb(var(--color-muted-foreground))] mt-1">
            {{ course.description }}
          </p>
        </div>
        <div class="flex gap-2">
          <AppButton
            v-if="!enrollment"
            :loading="enrolling"
            @click="enroll"
          >
            Enroll
          </AppButton>
          <AppButton
            v-else-if="enrollment.status === 'active'"
            variant="secondary"
            @click="router.push(`/learn/${course.id}`)"
          >
            Continue Learning
          </AppButton>
          <StatusBadge v-else :status="enrollment.status" />
        </div>
      </div>

      <!-- Chapters -->
      <div v-if="chapters.length > 0" class="card p-5 mb-6">
        <h2 class="text-base font-semibold mb-3">Chapters</h2>
        <div class="space-y-2">
          <div
            v-for="chapter in chapters"
            :key="chapter.id"
            class="flex items-center gap-3 p-2.5 rounded-md bg-[rgb(var(--color-muted)/0.3)]"
          >
            <span class="text-xs text-[rgb(var(--color-muted-foreground))] w-6 text-right font-mono">
              {{ chapter.position }}
            </span>
            <div>
              <div class="text-sm font-medium">{{ chapter.title }}</div>
              <div v-if="chapter.description" class="text-xs text-[rgb(var(--color-muted-foreground))]">
                {{ chapter.description }}
              </div>
            </div>
          </div>
        </div>
      </div>

      <!-- Details -->
      <div class="card p-5">
        <h2 class="text-base font-semibold mb-3">Details</h2>
        <div class="space-y-2">
          <DataRow label="Author" mono>{{ course.author_address }}</DataRow>
          <DataRow v-if="course.content_cid" label="Content CID" mono>{{ course.content_cid }}</DataRow>
          <DataRow v-if="course.tags?.length" label="Tags">{{ course.tags.join(', ') }}</DataRow>
          <DataRow v-if="course.skill_ids?.length" label="Skills">{{ course.skill_ids.length }} linked skill{{ course.skill_ids.length !== 1 ? 's' : '' }}</DataRow>
          <DataRow label="Created">{{ course.created_at }}</DataRow>
        </div>
      </div>
    </div>
  </div>
</template>
