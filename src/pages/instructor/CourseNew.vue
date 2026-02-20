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

    const course = await invoke<Course>('create_course', { request })
    router.push(`/instructor/courses/${course.id}`)
  } catch (e) {
    error.value = String(e)
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <div class="max-w-2xl">
    <h1 class="text-xl font-bold mb-1">Create Course</h1>
    <p class="text-sm text-[rgb(var(--color-muted-foreground))] mb-6">
      Create a new course on your local node. You can add chapters and elements after creation.
    </p>

    <div class="card p-5">
      <div class="space-y-4">
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

        <p v-if="error" class="text-sm text-[rgb(var(--color-error))]">{{ error }}</p>

        <div class="flex gap-2">
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
</template>
