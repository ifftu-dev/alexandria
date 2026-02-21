<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useLocalApi } from '@/composables/useLocalApi'
import { AppButton, AppSpinner, AppBadge, EmptyState, StatusBadge, ConfirmDialog } from '@/components/ui'
import type { Course, Chapter, Element, CreateChapterRequest, CreateElementRequest, PublishCourseResult, ElementSkillTag, SkillInfo } from '@/types'

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

// Skill tagging
const elementSkillTags = ref<Record<string, ElementSkillTag[]>>({})
const allSkills = ref<SkillInfo[]>([])
const taggingElement = ref<string | null>(null)
const skillSearch = ref('')
const skillSearchResults = computed<SkillInfo[]>(() => {
  const q = skillSearch.value.toLowerCase().trim()
  if (!q) return allSkills.value.slice(0, 20)
  return allSkills.value
    .filter(s => s.name.toLowerCase().includes(q) || (s.subject_name || '').toLowerCase().includes(q))
    .slice(0, 20)
})

function isSkillAlreadyTagged(elementId: string, skillId: string): boolean {
  return (elementSkillTags.value[elementId] || []).some(t => t.skill_id === skillId)
}

const bloomColors: Record<string, string> = {
  remember: 'var(--color-muted-foreground)',
  understand: '59 130 246',
  apply: '16 185 129',
  analyze: '245 158 11',
  evaluate: '239 68 68',
  create: '139 92 246',
}

onMounted(async () => {
  try {
    course.value = await invoke<Course>('get_course', { courseId })
    chapters.value = await invoke<Chapter[]>('list_chapters', { courseId })

    // Load elements for each chapter
    for (const ch of chapters.value) {
      elements.value[ch.id] = await invoke<Element[]>('list_elements', { chapterId: ch.id }).catch(() => [])

      // Load skill tags for each element
      for (const el of (elements.value[ch.id] ?? [])) {
        elementSkillTags.value[el.id] = await invoke<ElementSkillTag[]>('list_element_skill_tags', { elementId: el.id }).catch(() => [])
      }
    }

    // Load all skills for the picker
    allSkills.value = await invoke<SkillInfo[]>('list_skills', {}).catch(() => [])
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
    elementSkillTags.value[element.id] = []
    newElementTitle.value = ''
    newElementType.value = 'video'
    addingToChapter.value = null
  } catch (e) {
    error.value = String(e)
  } finally {
    creatingElement.value = false
  }
}

async function tagSkill(elementId: string, skill: SkillInfo) {
  if (isSkillAlreadyTagged(elementId, skill.id)) return
  try {
    await invoke('tag_element_skill', { elementId, skillId: skill.id })
    if (!elementSkillTags.value[elementId]) elementSkillTags.value[elementId] = []
    elementSkillTags.value[elementId].push({
      skill_id: skill.id,
      skill_name: skill.name,
      bloom_level: skill.bloom_level,
      weight: 1.0,
    })
    skillSearch.value = ''
    taggingElement.value = null
  } catch (e) {
    error.value = String(e)
  }
}

async function untagSkill(elementId: string, skillId: string) {
  try {
    await invoke('untag_element_skill', { elementId, skillId })
    elementSkillTags.value[elementId] = (elementSkillTags.value[elementId] || []).filter(t => t.skill_id !== skillId)
  } catch (e) {
    error.value = String(e)
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
  { value: 'text', label: 'Text' },
  { value: 'quiz', label: 'Quiz' },
  { value: 'interactive', label: 'Interactive' },
  { value: 'assessment', label: 'Assessment' },
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
          <div v-if="elements[chapter.id]?.length" class="space-y-2 mb-2">
            <div
              v-for="el in elements[chapter.id]"
              :key="el.id"
              class="p-2 rounded bg-[rgb(var(--color-muted)/0.3)]"
            >
              <div class="flex items-center gap-2 text-sm">
                <StatusBadge :status="el.element_type" />
                <span>{{ el.title }}</span>
                <span v-if="el.content_cid" class="text-xs font-mono text-[rgb(var(--color-muted-foreground))] ml-auto truncate max-w-40">
                  {{ el.content_cid }}
                </span>
              </div>

              <!-- Skill tags row -->
              <div class="flex flex-wrap items-center gap-1.5 mt-1.5">
                <button
                  v-for="tag in elementSkillTags[el.id] || []"
                  :key="tag.skill_id"
                  class="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-xs font-medium transition-colors hover:opacity-80"
                  :style="{ backgroundColor: `rgb(${bloomColors[tag.bloom_level] || bloomColors.apply} / 0.15)`, color: `rgb(${bloomColors[tag.bloom_level] || bloomColors.apply})` }"
                  :title="`Remove ${tag.skill_name}`"
                  @click="untagSkill(el.id, tag.skill_id)"
                >
                  {{ tag.skill_name }}
                  <svg class="w-3 h-3 opacity-60" viewBox="0 0 20 20" fill="currentColor"><path d="M6.28 5.22a.75.75 0 00-1.06 1.06L8.94 10l-3.72 3.72a.75.75 0 101.06 1.06L10 11.06l3.72 3.72a.75.75 0 101.06-1.06L11.06 10l3.72-3.72a.75.75 0 00-1.06-1.06L10 8.94 6.28 5.22z"/></svg>
                </button>

                <!-- Add skill button / picker -->
                <div v-if="taggingElement === el.id" class="relative">
                  <input
                    v-model="skillSearch"
                    class="input text-xs py-0.5 px-2 w-48"
                    placeholder="Search skills..."
                    @keydown.escape="taggingElement = null; skillSearch = ''"
                  >
                  <div
                    v-if="skillSearchResults.length"
                    class="absolute z-20 top-full left-0 mt-1 w-64 max-h-48 overflow-y-auto rounded border border-[rgb(var(--color-border))] bg-[rgb(var(--color-card))] shadow-lg"
                  >
                    <button
                      v-for="skill in skillSearchResults"
                      :key="skill.id"
                      class="w-full text-left px-3 py-1.5 text-xs hover:bg-[rgb(var(--color-muted)/0.3)] flex items-center justify-between gap-2 disabled:opacity-40"
                      :disabled="isSkillAlreadyTagged(el.id, skill.id)"
                      @click="tagSkill(el.id, skill)"
                    >
                      <span class="truncate">{{ skill.name }}</span>
                      <AppBadge size="xs">{{ skill.bloom_level }}</AppBadge>
                    </button>
                  </div>
                </div>
                <button
                  v-else
                  class="inline-flex items-center gap-0.5 px-1.5 py-0.5 rounded-full text-xs text-[rgb(var(--color-muted-foreground))] border border-dashed border-[rgb(var(--color-border))] hover:border-[rgb(var(--color-primary))] hover:text-[rgb(var(--color-primary))] transition-colors"
                  @click="taggingElement = el.id; skillSearch = ''"
                >
                  <svg class="w-3 h-3" viewBox="0 0 20 20" fill="currentColor"><path d="M10.75 4.75a.75.75 0 00-1.5 0v4.5h-4.5a.75.75 0 000 1.5h4.5v4.5a.75.75 0 001.5 0v-4.5h4.5a.75.75 0 000-1.5h-4.5v-4.5z"/></svg>
                  Skill
                </button>
              </div>
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
