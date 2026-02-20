<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useLocalApi } from '@/composables/useLocalApi'
import { AppButton, AppSpinner, EmptyState, StatusBadge, ConfirmDialog } from '@/components/ui'
import type { Course, Chapter, Element, CreateChapterRequest, CreateElementRequest, PublishCourseResult } from '@/types'

const { invoke } = useLocalApi()
const route = useRoute()
const router = useRouter()

const courseId = route.params.id as string

const course = ref<Course | null>(null)
const chapters = ref<Chapter[]>([])
const elements = ref<Record<string, Element[]>>({})
const loading = ref(true)
const error = ref('')

// Chapter creation
const showNewChapter = ref(false)
const newChapterTitle = ref('')
const creatingChapter = ref(false)

// Element creation
const addingToChapter = ref<string | null>(null)
const newElementTitle = ref('')
const newElementType = ref('video')
const creatingElement = ref(false)

// Publishing
const publishing = ref(false)
const publishResult = ref<PublishCourseResult | null>(null)

// Delete
const showDeleteConfirm = ref(false)
const deleting = ref(false)

onMounted(async () => {
  try {
    course.value = await invoke<Course>('get_course', { courseId })
    chapters.value = await invoke<Chapter[]>('list_chapters', { courseId })

    // Load elements for each chapter
    for (const ch of chapters.value) {
      elements.value[ch.id] = await invoke<Element[]>('list_elements', { chapterId: ch.id }).catch(() => [])
    }
  } catch (e) {
    error.value = String(e)
  } finally {
    loading.value = false
  }
})

async function addChapter() {
  if (!newChapterTitle.value.trim()) return
  creatingChapter.value = true
  try {
    const req: CreateChapterRequest = { title: newChapterTitle.value.trim() }
    const chapter = await invoke<Chapter>('create_chapter', { courseId, request: req })
    chapters.value.push(chapter)
    elements.value[chapter.id] = []
    newChapterTitle.value = ''
    showNewChapter.value = false
  } catch (e) {
    error.value = String(e)
  } finally {
    creatingChapter.value = false
  }
}

async function addElement(chapterId: string) {
  if (!newElementTitle.value.trim()) return
  creatingElement.value = true
  try {
    const req: CreateElementRequest = {
      title: newElementTitle.value.trim(),
      element_type: newElementType.value,
    }
    const element = await invoke<Element>('create_element', { chapterId, request: req })
    if (!elements.value[chapterId]) elements.value[chapterId] = []
    elements.value[chapterId].push(element)
    newElementTitle.value = ''
    newElementType.value = 'video'
    addingToChapter.value = null
  } catch (e) {
    error.value = String(e)
  } finally {
    creatingElement.value = false
  }
}

async function publishCourse() {
  publishing.value = true
  error.value = ''
  try {
    publishResult.value = await invoke<PublishCourseResult>('publish_course', { courseId })
    course.value = await invoke<Course>('get_course', { courseId })
  } catch (e) {
    error.value = String(e)
  } finally {
    publishing.value = false
  }
}

async function deleteCourse() {
  deleting.value = true
  try {
    await invoke('delete_course', { courseId })
    router.replace('/courses')
  } catch (e) {
    error.value = String(e)
  } finally {
    deleting.value = false
    showDeleteConfirm.value = false
  }
}

const elementTypes = [
  { value: 'video', label: 'Video' },
  { value: 'pdf', label: 'PDF' },
  { value: 'quiz', label: 'Quiz (MCQ)' },
  { value: 'essay', label: 'Essay' },
  { value: 'download', label: 'Download' },
]
</script>

<template>
  <div>
    <AppSpinner v-if="loading" label="Loading course..." />

    <EmptyState v-else-if="!course" title="Course not found" />

    <div v-else>
      <!-- Header -->
      <div class="flex items-start justify-between mb-6">
        <div>
          <div class="flex items-center gap-2 mb-1">
            <StatusBadge :status="course.status" />
            <span class="text-xs text-[rgb(var(--color-muted-foreground))]">v{{ course.version }}</span>
          </div>
          <h1 class="text-xl font-bold">{{ course.title }}</h1>
        </div>
        <div class="flex gap-2">
          <AppButton
            v-if="course.status === 'draft'"
            :loading="publishing"
            @click="publishCourse"
          >
            Publish to IPFS
          </AppButton>
          <AppButton
            variant="danger"
            size="sm"
            @click="showDeleteConfirm = true"
          >
            Delete
          </AppButton>
        </div>
      </div>

      <!-- Publish result -->
      <div v-if="publishResult" class="alert alert-success mb-6">
        Published! Content hash: <code class="font-mono text-xs ml-1">{{ publishResult.content_hash }}</code>
        ({{ publishResult.size }} bytes)
      </div>

      <p v-if="error" class="text-sm text-[rgb(var(--color-error))] mb-4">{{ error }}</p>

      <!-- Chapters -->
      <div class="space-y-4">
        <div v-for="chapter in chapters" :key="chapter.id" class="card p-4">
          <div class="flex items-center justify-between mb-3">
            <div class="flex items-center gap-2">
              <span class="text-xs font-mono text-[rgb(var(--color-muted-foreground))]">{{ chapter.position }}</span>
              <h3 class="text-sm font-semibold">{{ chapter.title }}</h3>
            </div>
            <AppButton
              variant="ghost"
              size="xs"
              @click="addingToChapter = addingToChapter === chapter.id ? null : chapter.id; newElementTitle = ''"
            >
              + Element
            </AppButton>
          </div>

          <!-- Elements -->
          <div v-if="elements[chapter.id]?.length" class="space-y-1 mb-2">
            <div
              v-for="el in elements[chapter.id]"
              :key="el.id"
              class="flex items-center gap-2 p-2 rounded bg-[rgb(var(--color-muted)/0.3)] text-sm"
            >
              <StatusBadge :status="el.element_type" />
              <span>{{ el.title }}</span>
              <span v-if="el.content_cid" class="text-xs font-mono text-[rgb(var(--color-muted-foreground))] ml-auto truncate max-w-40">
                {{ el.content_cid }}
              </span>
            </div>
          </div>

          <!-- Add element form -->
          <div v-if="addingToChapter === chapter.id" class="mt-2 p-3 rounded bg-[rgb(var(--color-muted)/0.2)]">
            <div class="flex gap-2 mb-2">
              <input
                v-model="newElementTitle"
                class="input flex-1"
                placeholder="Element title"
                @keydown.enter="addElement(chapter.id)"
              >
              <select
                v-model="newElementType"
                class="input w-32"
              >
                <option v-for="t in elementTypes" :key="t.value" :value="t.value">{{ t.label }}</option>
              </select>
            </div>
            <div class="flex gap-2">
              <AppButton size="sm" :loading="creatingElement" @click="addElement(chapter.id)">
                Add
              </AppButton>
              <AppButton variant="ghost" size="sm" @click="addingToChapter = null">
                Cancel
              </AppButton>
            </div>
          </div>
        </div>

        <!-- Add chapter -->
        <div v-if="showNewChapter" class="card p-4">
          <div class="flex gap-2">
            <input
              v-model="newChapterTitle"
              class="input flex-1"
              placeholder="Chapter title"
              @keydown.enter="addChapter"
            >
            <AppButton size="sm" :loading="creatingChapter" @click="addChapter">
              Add Chapter
            </AppButton>
            <AppButton variant="ghost" size="sm" @click="showNewChapter = false">
              Cancel
            </AppButton>
          </div>
        </div>
        <AppButton
          v-else
          variant="outline"
          @click="showNewChapter = true"
        >
          + Add Chapter
        </AppButton>
      </div>

      <!-- Delete confirm -->
      <ConfirmDialog
        :open="showDeleteConfirm"
        title="Delete Course"
        message="This will permanently delete this course and all its chapters and elements. This cannot be undone."
        confirm-label="Delete"
        confirm-variant="danger"
        :loading="deleting"
        @confirm="deleteCourse"
        @cancel="showDeleteConfirm = false"
      />
    </div>
  </div>
</template>
