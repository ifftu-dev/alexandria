<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useLocalApi } from '@/composables/useLocalApi'
import { AppButton, AppBadge, StatusBadge, ConfirmDialog } from '@/components/ui'
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

// Stats
const totalChapters = computed(() => chapters.value.length)
const totalElements = computed(() => {
  let count = 0
  for (const elems of Object.values(elements.value)) count += elems.length
  return count
})
const totalSkillTags = computed(() => {
  let count = 0
  for (const tags of Object.values(elementSkillTags.value)) count += tags.length
  return count
})

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
    const chapter = await invoke<Chapter>('create_chapter', { courseId, req })
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
    const element = await invoke<Element>('create_element', { chapterId, req })
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
  { value: 'pdf', label: 'PDF' },
  { value: 'downloadable', label: 'Download' },
  { value: 'quiz', label: 'Quiz' },
  { value: 'objective_single_mcq', label: 'Single MCQ' },
  { value: 'objective_multi_mcq', label: 'Multi MCQ' },
  { value: 'subjective_mcq', label: 'Subjective MCQ' },
  { value: 'essay', label: 'Essay' },
  { value: 'interactive', label: 'Interactive' },
  { value: 'assessment', label: 'Assessment' },
]

function elementTypeIcon(type: string): string {
  switch (type) {
    case 'video': return 'M14.752 11.168l-3.197-2.132A1 1 0 0010 9.87v4.263a1 1 0 001.555.832l3.197-2.132a1 1 0 000-1.664z'
    case 'text': return 'M4 6h16M4 12h16M4 18h7'
    case 'pdf': return 'M7 21h10a2 2 0 002-2V9.414a1 1 0 00-.293-.707l-5.414-5.414A1 1 0 0012.586 3H7a2 2 0 00-2 2v14a2 2 0 002 2z'
    case 'downloadable': return 'M4 16v1a3 3 0 003 3h10a3 3 0 003-3v-1m-4-4l-4 4m0 0l-4-4m4 4V4'
    case 'quiz': case 'assessment': return 'M9 5H7a2 2 0 00-2 2v12a2 2 0 002 2h10a2 2 0 002-2V7a2 2 0 00-2-2h-2M9 5a2 2 0 002 2h2a2 2 0 002-2M9 5a2 2 0 012-2h2a2 2 0 012 2'
    case 'objective_single_mcq': case 'objective_multi_mcq': case 'subjective_mcq': return 'M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z'
    case 'essay': return 'M11 5H6a2 2 0 00-2 2v11a2 2 0 002 2h11a2 2 0 002-2v-5m-1.414-9.414a2 2 0 112.828 2.828L11.828 15H9v-2.828l8.586-8.586z'
    case 'interactive': return 'M13 10V3L4 14h7v7l9-11h-7z'
    default: return 'M7 21h10a2 2 0 002-2V9.414a1 1 0 00-.293-.707l-5.414-5.414A1 1 0 0012.586 3H7a2 2 0 00-2 2v14a2 2 0 002 2z'
  }
}
</script>

<template>
  <div>
    <!-- Skeleton -->
    <div v-if="loading" class="space-y-6">
      <div class="flex items-start justify-between">
        <div class="space-y-2">
          <div class="flex items-center gap-2">
            <div class="h-5 w-14 animate-pulse rounded-full bg-[rgb(var(--color-muted-foreground)/0.15)]" />
            <div class="h-4 w-8 animate-pulse rounded bg-[rgb(var(--color-muted-foreground)/0.1)]" />
          </div>
          <div class="h-7 w-64 animate-pulse rounded bg-[rgb(var(--color-muted-foreground)/0.2)]" />
        </div>
        <div class="flex gap-2">
          <div class="h-9 w-28 animate-pulse rounded-lg bg-[rgb(var(--color-muted-foreground)/0.15)]" />
        </div>
      </div>
      <div class="grid grid-cols-1 sm:grid-cols-3 gap-4">
        <div v-for="i in 3" :key="i" class="animate-pulse rounded-xl border border-[rgb(var(--color-border))] bg-[rgb(var(--color-card))] p-5">
          <div class="h-3 w-16 rounded bg-[rgb(var(--color-muted-foreground)/0.15)] mb-2" />
          <div class="h-7 w-8 rounded bg-[rgb(var(--color-muted-foreground)/0.2)]" />
        </div>
      </div>
      <div v-for="i in 2" :key="i" class="animate-pulse rounded-xl border border-[rgb(var(--color-border))] bg-[rgb(var(--color-card))] p-5">
        <div class="flex items-center gap-3 mb-3">
          <div class="h-6 w-6 rounded bg-[rgb(var(--color-muted-foreground)/0.15)]" />
          <div class="h-4 w-32 rounded bg-[rgb(var(--color-muted-foreground)/0.15)]" />
        </div>
        <div class="space-y-2">
          <div v-for="j in 3" :key="j" class="h-10 rounded-lg bg-[rgb(var(--color-muted-foreground)/0.08)]" />
        </div>
      </div>
    </div>

    <!-- Not found -->
    <div v-else-if="!course" class="py-16 text-center">
      <div class="mx-auto mb-4 flex h-16 w-16 items-center justify-center rounded-full bg-[rgb(var(--color-muted)/0.3)]">
        <svg class="h-8 w-8 text-[rgb(var(--color-muted-foreground)/0.5)]" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
          <path stroke-linecap="round" stroke-linejoin="round" d="M12 6.253v13m0-13C10.832 5.477 9.246 5 7.5 5S4.168 5.477 3 6.253v13C4.168 18.477 5.754 18 7.5 18s3.332.477 4.5 1.253m0-13C13.168 5.477 14.754 5 16.5 5c1.747 0 3.332.477 4.5 1.253v13C19.832 18.477 18.247 18 16.5 18c-1.746 0-3.332.477-4.5 1.253" />
        </svg>
      </div>
      <h2 class="text-lg font-semibold">Course not found</h2>
    </div>

    <div v-else class="space-y-6">
      <!-- Header -->
      <div class="flex items-start justify-between">
        <div>
          <div class="flex items-center gap-2 mb-2">
            <StatusBadge :status="course.status" />
            <span class="text-xs text-[rgb(var(--color-muted-foreground))]">v{{ course.version }}</span>
          </div>
          <h1 class="text-2xl font-bold text-[rgb(var(--color-foreground))]">{{ course.title }}</h1>
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

      <!-- Stats -->
      <div class="grid grid-cols-1 sm:grid-cols-3 gap-4">
        <div class="rounded-xl border border-[rgb(var(--color-border))] bg-[rgb(var(--color-card))] p-5">
          <p class="text-xs text-[rgb(var(--color-muted-foreground))]">Chapters</p>
          <p class="mt-1 text-2xl font-bold text-[rgb(var(--color-foreground))]">{{ totalChapters }}</p>
        </div>
        <div class="rounded-xl border border-[rgb(var(--color-border))] bg-[rgb(var(--color-card))] p-5">
          <p class="text-xs text-[rgb(var(--color-muted-foreground))]">Elements</p>
          <p class="mt-1 text-2xl font-bold text-[rgb(var(--color-primary))]">{{ totalElements }}</p>
        </div>
        <div class="rounded-xl border border-[rgb(var(--color-border))] bg-[rgb(var(--color-card))] p-5">
          <p class="text-xs text-[rgb(var(--color-muted-foreground))]">Skill Tags</p>
          <p class="mt-1 text-2xl font-bold text-[rgb(var(--color-foreground))]">{{ totalSkillTags }}</p>
        </div>
      </div>

      <!-- Publish result -->
      <div v-if="publishResult" class="rounded-lg border border-emerald-500/20 bg-emerald-500/10 p-4 flex items-center gap-3">
        <svg class="h-5 w-5 text-emerald-500 flex-shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
          <path stroke-linecap="round" stroke-linejoin="round" d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z" />
        </svg>
        <div>
          <p class="text-sm font-medium text-emerald-700 dark:text-emerald-300">Published!</p>
          <p class="text-xs text-emerald-600 dark:text-emerald-400">
            Hash: <code class="font-mono">{{ publishResult.content_hash }}</code> ({{ publishResult.size }} bytes)
          </p>
        </div>
      </div>

      <p v-if="error" class="text-sm text-red-600 dark:text-red-400">{{ error }}</p>

      <!-- Chapters -->
      <div class="space-y-4">
        <div
          v-for="(chapter, chIdx) in chapters"
          :key="chapter.id"
          class="rounded-xl border border-[rgb(var(--color-border))] bg-[rgb(var(--color-card))] p-5"
        >
          <!-- Chapter header -->
          <div class="flex items-center justify-between mb-4">
            <div class="flex items-center gap-3">
              <span class="flex h-7 w-7 items-center justify-center rounded-lg bg-[rgb(var(--color-primary)/0.1)] text-xs font-bold text-[rgb(var(--color-primary))]">
                {{ chIdx + 1 }}
              </span>
              <h3 class="text-sm font-semibold text-[rgb(var(--color-foreground))]">{{ chapter.title }}</h3>
              <span class="text-xs text-[rgb(var(--color-muted-foreground))]">
                {{ (elements[chapter.id] ?? []).length }} element{{ (elements[chapter.id] ?? []).length !== 1 ? 's' : '' }}
              </span>
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
          <div v-if="elements[chapter.id]?.length" class="space-y-2 mb-3">
            <div
              v-for="el in elements[chapter.id]"
              :key="el.id"
              class="rounded-lg border border-[rgb(var(--color-border)/0.5)] bg-[rgb(var(--color-muted)/0.15)] p-3"
            >
              <div class="flex items-center gap-2.5 text-sm">
                <svg class="h-4 w-4 flex-shrink-0 text-[rgb(var(--color-muted-foreground))]" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                  <path stroke-linecap="round" stroke-linejoin="round" :d="elementTypeIcon(el.element_type)" />
                </svg>
                <span class="font-medium text-[rgb(var(--color-foreground))]">{{ el.title }}</span>
                <StatusBadge :status="el.element_type" />
                <span v-if="el.content_cid" class="ml-auto text-xs font-mono text-[rgb(var(--color-muted-foreground))] truncate max-w-40">
                  {{ el.content_cid }}
                </span>
              </div>

              <!-- Skill tags row -->
              <div class="flex flex-wrap items-center gap-1.5 mt-2">
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
                    class="rounded-lg border border-[rgb(var(--color-border))] bg-[rgb(var(--color-background))] px-2 py-1 text-xs w-48"
                    placeholder="Search skills..."
                    @keydown.escape="taggingElement = null; skillSearch = ''"
                  >
                  <div
                    v-if="skillSearchResults.length"
                    class="absolute z-20 top-full left-0 mt-1 w-64 max-h-48 overflow-y-auto rounded-lg border border-[rgb(var(--color-border))] bg-[rgb(var(--color-card))] shadow-lg"
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
          <div v-if="addingToChapter === chapter.id" class="mt-3 rounded-lg border border-dashed border-[rgb(var(--color-border))] bg-[rgb(var(--color-muted)/0.1)] p-4">
            <div class="flex gap-2 mb-3">
              <input
                v-model="newElementTitle"
                class="flex-1 rounded-lg border border-[rgb(var(--color-border))] bg-[rgb(var(--color-background))] px-3 py-2 text-sm"
                placeholder="Element title"
                @keydown.enter="addElement(chapter.id)"
              >
              <select
                v-model="newElementType"
                class="rounded-lg border border-[rgb(var(--color-border))] bg-[rgb(var(--color-background))] px-3 py-2 text-sm"
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
        <div v-if="showNewChapter" class="rounded-xl border border-dashed border-[rgb(var(--color-border))] bg-[rgb(var(--color-card))] p-5">
          <div class="flex gap-2">
            <input
              v-model="newChapterTitle"
              class="flex-1 rounded-lg border border-[rgb(var(--color-border))] bg-[rgb(var(--color-background))] px-3 py-2 text-sm"
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
