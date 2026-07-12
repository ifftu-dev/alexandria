<script setup lang="ts">
// Instructor's authored courses & tutorials, with entry points into
// the composer.
import { computed, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { useLocalApi } from '@/composables/useLocalApi'
import { useProfiles } from '@/composables/useProfiles'
import { AppButton, EmptyState, StatusBadge } from '@/components/ui'
import type { Course } from '@/types'

const { invoke } = useLocalApi()
const router = useRouter()
const { stakeAddress } = useProfiles()

const courses = ref<Course[]>([])
const loading = ref(true)

const mine = computed(() =>
  courses.value
    .filter(c => c.author_address === stakeAddress.value)
    .sort((a, b) => b.updated_at.localeCompare(a.updated_at)),
)

onMounted(async () => {
  try {
    courses.value = await invoke<Course[]>('list_courses', { status: null })
  } finally {
    loading.value = false
  }
})
</script>

<template>
  <div class="space-y-6">
    <div class="flex items-start justify-between gap-4">
      <div>
        <h1 class="text-2xl font-bold text-foreground">{{ $t('instructor.myCourses.title') }}</h1>
        <p class="mt-1 text-sm text-muted-foreground">
          {{ $t('instructor.myCourses.subtitle') }}
        </p>
      </div>
      <div class="flex shrink-0 gap-2">
        <AppButton size="sm" @click="router.push('/instructor/composer/new?kind=course')">
          {{ $t('instructor.myCourses.addCourse') }}
        </AppButton>
        <AppButton variant="outline" size="sm" @click="router.push('/instructor/composer/new?kind=tutorial')">
          {{ $t('instructor.myCourses.addTutorial') }}
        </AppButton>
      </div>
    </div>

    <div v-if="loading" class="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
      <div v-for="i in 6" :key="i" class="h-32 animate-pulse rounded-xl bg-muted-foreground/8" />
    </div>

    <EmptyState
      v-else-if="!mine.length"
      :title="$t('instructor.myCourses.emptyTitle')"
      :description="$t('instructor.myCourses.emptyDesc')"
    />

    <div v-else class="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
      <button
        v-for="course in mine"
        :key="course.id"
        class="rounded-xl border border-border bg-card p-5 text-start transition-colors hover:border-primary/50"
        @click="router.push(`/instructor/composer/${course.id}`)"
      >
        <div class="mb-2 flex items-center gap-2">
          <StatusBadge :status="course.status" />
          <span class="rounded-full bg-muted px-2 py-0.5 text-[10px] font-medium uppercase tracking-wide text-muted-foreground">
            {{ course.kind }}
          </span>
        </div>
        <h3 class="font-semibold text-foreground line-clamp-1">{{ course.title }}</h3>
        <p v-if="course.description" class="mt-1 text-sm text-muted-foreground line-clamp-2">
          {{ course.description }}
        </p>
        <p class="mt-2 text-xs text-muted-foreground">{{ $t('instructor.myCourses.updated', { date: course.updated_at.slice(0, 10) }) }}</p>
      </button>
    </div>
  </div>
</template>
