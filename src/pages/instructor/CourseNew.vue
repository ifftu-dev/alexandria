<script setup lang="ts">
import { ref } from 'vue'
import { useRouter } from 'vue-router'
import { useLocalApi } from '@/composables/useLocalApi'
import { AppButton, AppInput, AppTextarea } from '@/components/ui'
import type { Course, CreateCourseRequest } from '@/types'

const { invoke } = useLocalApi()
const router = useRouter()

const title = ref('')
const description = ref('')
const tags = ref('')
const saving = ref(false)
const error = ref('')

async function create() {
  if (!title.value.trim()) {
    error.value = 'Title is required.'
    return
  }

  saving.value = true
  error.value = ''

  try {
    const request: CreateCourseRequest = {
      title: title.value.trim(),
      description: description.value.trim() || null,
      tags: tags.value.trim() ? tags.value.split(',').map(t => t.trim()).filter(Boolean) : null,
    }

    const course = await invoke<Course>('create_course', { req: request })
    router.push(`/instructor/courses/${course.id}`)
  } catch (e) {
    error.value = String(e)
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <div>
    <div class="max-w-2xl">
      <div class="mb-8">
        <h1 class="text-3xl font-bold text-foreground">Create Course</h1>
        <p class="mt-2 text-muted-foreground">
          Create a new course on your local node. You can add chapters and elements after creation.
        </p>
      </div>

      <div class="rounded-xl border border-border bg-card p-6">
        <div class="space-y-5">
          <AppInput
            v-model="title"
            label="Title"
            placeholder="e.g., Introduction to Category Theory"
          />

          <AppTextarea
            v-model="description"
            label="Description"
            placeholder="What will learners gain from this course?"
            :rows="4"
          />

          <AppInput
            v-model="tags"
            label="Tags (comma-separated)"
            placeholder="e.g., mathematics, algebra, category-theory"
          />

          <p v-if="error" class="text-sm text-red-600 dark:text-red-400">{{ error }}</p>

          <div class="flex gap-3 pt-2">
            <AppButton :loading="saving" @click="create">
              Create Course
            </AppButton>
            <AppButton variant="ghost" @click="router.back()">
              Cancel
            </AppButton>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>
