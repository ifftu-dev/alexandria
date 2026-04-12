<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useRouter } from 'vue-router'
import { invoke } from '@tauri-apps/api/core'
import { AppButton, AppInput, AppTextarea, AppAlert, AppBadge } from '@/components/ui'
import type {
  Course,
  TaxonomySkill,
  PublishTutorialRequest,
  PublishCourseResult,
  VideoChapterInput,
  SkillTagInput,
} from '@/types'

// -----------------------------------------------------------------------------
// Form state
// -----------------------------------------------------------------------------

const router = useRouter()

const title = ref('')
const description = ref('')
const tagsCsv = ref('')

const videoFile = ref<File | null>(null)
const videoHash = ref<string | null>(null)
const videoDurationSeconds = ref<number | null>(null)
const videoUploading = ref(false)
const videoUploadProgress = ref<string>('') // short status text

const thumbFile = ref<File | null>(null)
const thumbHash = ref<string | null>(null)
const thumbUploading = ref(false)

// Skill tags — min 1 required. Each row: skill_id + optional weight.
const skillTags = ref<SkillTagInput[]>([])
const skillsCatalog = ref<TaxonomySkill[]>([])

// Optional timestamp markers.
const chapters = ref<VideoChapterInput[]>([])

// Optional end-of-video quiz.
const includeQuiz = ref(false)
const quizJson = ref<string>(
  // Starter template the author can edit. Matches the inline quiz
  // format used by QuizEngine.vue.
  JSON.stringify(
    {
      questions: [
        {
          id: 'q1',
          question: 'Write your first question here.',
          options: ['Option A', 'Option B', 'Option C'],
          correct_index: 0,
        },
      ],
    },
    null,
    2,
  ),
)

const submitting = ref(false)
const error = ref('')

// -----------------------------------------------------------------------------
// Derived state
// -----------------------------------------------------------------------------

const canSubmit = computed(
  () =>
    title.value.trim().length > 0 &&
    videoHash.value !== null &&
    skillTags.value.length > 0 &&
    skillTags.value.every((t) => t.skill_id.length > 0) &&
    !submitting.value &&
    !videoUploading.value &&
    !thumbUploading.value,
)

const quizJsonValid = computed(() => {
  if (!includeQuiz.value) return true
  try {
    const parsed = JSON.parse(quizJson.value)
    return (
      parsed &&
      Array.isArray(parsed.questions) &&
      parsed.questions.length > 0 &&
      parsed.questions.every(
        (q: unknown) =>
          typeof q === 'object' &&
          q !== null &&
          'question' in q &&
          'options' in q &&
          'correct_index' in q,
      )
    )
  } catch {
    return false
  }
})

// -----------------------------------------------------------------------------
// Mount — fetch skills taxonomy for the tag picker
// -----------------------------------------------------------------------------

onMounted(async () => {
  try {
    skillsCatalog.value = await invoke<TaxonomySkill[]>('list_skills', {
      subjectId: null,
      search: null,
    })
  } catch (e) {
    console.warn('Failed to load skills taxonomy:', e)
  }
})

// -----------------------------------------------------------------------------
// File uploads via content_add
// -----------------------------------------------------------------------------

async function readFileAsBytes(file: File): Promise<number[]> {
  const buf = await file.arrayBuffer()
  // Tauri IPC serialises Uint8Array as JSON arrays of numbers. For very
  // large videos this is expensive — fine for a dev/demo flow, a future
  // optimisation would expose a streaming command that takes a file path.
  return Array.from(new Uint8Array(buf))
}

async function uploadVideo(file: File) {
  videoUploading.value = true
  error.value = ''
  videoUploadProgress.value = `Reading ${Math.round(file.size / 1024 / 1024)} MB…`
  try {
    const bytes = await readFileAsBytes(file)
    videoUploadProgress.value = 'Adding to iroh…'
    const result = await invoke<{ hash: string; size: number }>('content_add', {
      data: bytes,
    })
    videoHash.value = result.hash
    // Try to probe duration via an off-screen <video> element. Best-effort —
    // `duration_seconds` is optional on the payload.
    const probe = document.createElement('video')
    probe.preload = 'metadata'
    probe.src = URL.createObjectURL(file)
    await new Promise<void>((resolve) => {
      probe.onloadedmetadata = () => resolve()
      probe.onerror = () => resolve()
    })
    if (Number.isFinite(probe.duration) && probe.duration > 0) {
      videoDurationSeconds.value = Math.round(probe.duration)
    }
    URL.revokeObjectURL(probe.src)
    videoUploadProgress.value = `Uploaded (${result.hash.slice(0, 12)}…)`
  } catch (e) {
    error.value = `Video upload failed: ${e}`
    videoHash.value = null
  } finally {
    videoUploading.value = false
  }
}

async function uploadThumb(file: File) {
  thumbUploading.value = true
  try {
    const bytes = await readFileAsBytes(file)
    const result = await invoke<{ hash: string }>('content_add', { data: bytes })
    thumbHash.value = result.hash
  } catch (e) {
    error.value = `Thumbnail upload failed: ${e}`
  } finally {
    thumbUploading.value = false
  }
}

function onVideoChange(e: Event) {
  const input = e.target as HTMLInputElement
  const file = input.files?.[0]
  if (!file) return
  videoFile.value = file
  uploadVideo(file)
}

function onThumbChange(e: Event) {
  const input = e.target as HTMLInputElement
  const file = input.files?.[0]
  if (!file) return
  thumbFile.value = file
  uploadThumb(file)
}

// -----------------------------------------------------------------------------
// Skill tags / chapters helpers
// -----------------------------------------------------------------------------

function addSkillTag() {
  skillTags.value.push({ skill_id: '', weight: 1.0 })
}
function removeSkillTag(i: number) {
  skillTags.value.splice(i, 1)
}

function addChapter() {
  chapters.value.push({ title: '', start_seconds: 0 })
}
function removeChapter(i: number) {
  chapters.value.splice(i, 1)
}

// -----------------------------------------------------------------------------
// Submit
// -----------------------------------------------------------------------------

async function submit() {
  if (!canSubmit.value) return
  if (!videoHash.value) {
    error.value = 'Video upload is required.'
    return
  }
  if (includeQuiz.value && !quizJsonValid.value) {
    error.value = 'Quiz JSON is invalid — fix the editor or turn off the quiz.'
    return
  }

  submitting.value = true
  error.value = ''

  const req: PublishTutorialRequest = {
    title: title.value.trim(),
    description: description.value.trim() || null,
    video_content_hash: videoHash.value,
    thumbnail_hash: thumbHash.value,
    duration_seconds: videoDurationSeconds.value,
    skill_tags: skillTags.value
      .filter((t) => t.skill_id.length > 0)
      .map((t) => ({
        skill_id: t.skill_id,
        weight: t.weight ?? 1.0,
      })),
    video_chapters: chapters.value
      .filter((c) => c.title.trim().length > 0)
      .map((c) => ({ title: c.title.trim(), start_seconds: Math.max(0, c.start_seconds) })),
    quiz: includeQuiz.value
      ? { content_json: quizJson.value }
      : null,
    tags: tagsCsv.value
      .split(',')
      .map((t) => t.trim())
      .filter(Boolean),
  }

  try {
    const result = await invoke<PublishCourseResult>('publish_tutorial', { req })
    // The tutorial is just a course row — navigate to its detail page.
    const list = await invoke<Course[]>('list_courses', { status: null })
    // Freshly-published tutorial will be the most recent row with kind='tutorial'
    // matching the content_cid from `result.content_hash`. Fall back to /courses
    // if we can't pin down the row.
    const match =
      list.find((c) => c.content_cid === result.content_hash) ??
      list
        .filter((c) => c.kind === 'tutorial')
        .sort((a, b) => b.updated_at.localeCompare(a.updated_at))[0]
    if (match) {
      router.push(`/courses/${match.id}`)
    } else {
      router.push('/courses')
    }
  } catch (e) {
    error.value = `Publish failed: ${e}`
  } finally {
    submitting.value = false
  }
}
</script>

<template>
  <div class="max-w-3xl">
    <div class="mb-8">
      <h1 class="text-3xl font-bold text-foreground">Create Tutorial</h1>
      <p class="mt-2 text-muted-foreground">
        A tutorial is a single video with skill tags. Optional chapters mark
        timestamps, and an optional end-of-video quiz can contribute partial
        evidence toward the tagged skills.
      </p>
    </div>

    <div class="space-y-6">
      <!-- Basic metadata -->
      <section class="rounded-xl border border-border bg-card p-6 space-y-5">
        <h2 class="text-sm font-semibold uppercase tracking-wider text-muted-foreground">
          Basics
        </h2>

        <AppInput
          v-model="title"
          label="Title"
          placeholder="e.g., Big-O analysis — 8 minute explainer"
        />

        <AppTextarea
          v-model="description"
          label="Description"
          placeholder="One-paragraph pitch."
          :rows="3"
        />

        <AppInput
          v-model="tagsCsv"
          label="Tags (comma-separated)"
          placeholder="e.g., complexity, algorithms, intro"
        />
      </section>

      <!-- Video upload -->
      <section class="rounded-xl border border-border bg-card p-6 space-y-4">
        <h2 class="text-sm font-semibold uppercase tracking-wider text-muted-foreground">
          Video
        </h2>
        <label class="block">
          <span class="sr-only">Video file</span>
          <input
            type="file"
            accept="video/*"
            class="block w-full text-sm text-muted-foreground file:mr-4 file:rounded-md file:border-0 file:bg-primary/10 file:px-4 file:py-2 file:text-sm file:font-semibold file:text-primary hover:file:bg-primary/15 cursor-pointer"
            :disabled="videoUploading"
            @change="onVideoChange"
          />
        </label>
        <div v-if="videoUploading" class="text-sm text-muted-foreground">
          {{ videoUploadProgress }}
        </div>
        <div v-else-if="videoHash" class="flex items-center gap-2 text-sm">
          <AppBadge variant="success">Uploaded</AppBadge>
          <code class="text-xs text-muted-foreground">{{ videoHash.slice(0, 24) }}…</code>
          <span v-if="videoDurationSeconds" class="text-xs text-muted-foreground">
            · {{ Math.round(videoDurationSeconds / 60) }} min
          </span>
        </div>

        <label class="block pt-2">
          <span class="mb-1 block text-sm font-medium text-foreground">
            Thumbnail (optional)
          </span>
          <input
            type="file"
            accept="image/*"
            class="block w-full text-sm text-muted-foreground file:mr-4 file:rounded-md file:border-0 file:bg-muted file:px-3 file:py-1.5 file:text-sm file:text-foreground hover:file:bg-muted/80 cursor-pointer"
            :disabled="thumbUploading"
            @change="onThumbChange"
          />
          <span v-if="thumbHash" class="mt-1 block text-xs text-muted-foreground">
            Uploaded: {{ thumbHash.slice(0, 16) }}…
          </span>
        </label>
      </section>

      <!-- Skill tags -->
      <section class="rounded-xl border border-border bg-card p-6 space-y-4">
        <div class="flex items-center justify-between">
          <h2 class="text-sm font-semibold uppercase tracking-wider text-muted-foreground">
            Skill tags
          </h2>
          <AppButton variant="ghost" size="sm" @click="addSkillTag">
            Add skill
          </AppButton>
        </div>
        <p class="text-xs text-muted-foreground">
          At least one skill tag is required. Tutorials without skill tags are
          just videos — they need to feed something.
        </p>

        <div v-for="(tag, i) in skillTags" :key="i" class="flex items-center gap-2">
          <select
            v-model="tag.skill_id"
            class="flex-1 rounded-md border border-border bg-background px-3 py-2 text-sm"
          >
            <option value="">Pick a skill…</option>
            <option
              v-for="skill in skillsCatalog"
              :key="skill.id"
              :value="skill.id"
            >
              {{ skill.name }}
              <template v-if="skill.bloom_level">({{ skill.bloom_level }})</template>
            </option>
          </select>
          <input
            v-model.number="tag.weight"
            type="number"
            min="0"
            max="1"
            step="0.1"
            class="w-20 rounded-md border border-border bg-background px-2 py-2 text-sm text-center"
            title="Weight (0.0–1.0)"
          />
          <button
            type="button"
            class="rounded-md p-2 text-muted-foreground hover:bg-muted hover:text-foreground"
            @click="removeSkillTag(i)"
          >
            <svg class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
              <path stroke-linecap="round" stroke-linejoin="round" d="M6 18L18 6M6 6l12 12" />
            </svg>
          </button>
        </div>
      </section>

      <!-- Chapters -->
      <section class="rounded-xl border border-border bg-card p-6 space-y-4">
        <div class="flex items-center justify-between">
          <h2 class="text-sm font-semibold uppercase tracking-wider text-muted-foreground">
            Chapters (optional)
          </h2>
          <AppButton variant="ghost" size="sm" @click="addChapter">
            Add chapter
          </AppButton>
        </div>
        <p class="text-xs text-muted-foreground">
          Timestamps become clickable markers below the video.
        </p>

        <div v-for="(ch, i) in chapters" :key="i" class="flex items-center gap-2">
          <input
            v-model="ch.title"
            type="text"
            placeholder="Chapter title"
            class="flex-1 rounded-md border border-border bg-background px-3 py-2 text-sm"
          />
          <input
            v-model.number="ch.start_seconds"
            type="number"
            min="0"
            class="w-28 rounded-md border border-border bg-background px-2 py-2 text-sm text-center"
            title="Start (seconds)"
          />
          <button
            type="button"
            class="rounded-md p-2 text-muted-foreground hover:bg-muted hover:text-foreground"
            @click="removeChapter(i)"
          >
            <svg class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
              <path stroke-linecap="round" stroke-linejoin="round" d="M6 18L18 6M6 6l12 12" />
            </svg>
          </button>
        </div>
      </section>

      <!-- Optional quiz -->
      <section class="rounded-xl border border-border bg-card p-6 space-y-4">
        <label class="flex items-start gap-3 cursor-pointer">
          <input v-model="includeQuiz" type="checkbox" class="mt-1" />
          <div>
            <div class="text-sm font-medium text-foreground">
              Include an end-of-video check
            </div>
            <p class="mt-0.5 text-xs text-muted-foreground">
              A quiz passed by the learner contributes partial evidence toward
              the tagged skills (at a lower trust factor than a full course
              assessment). Without a quiz, the tutorial is purely informational.
            </p>
          </div>
        </label>

        <div v-if="includeQuiz" class="space-y-2">
          <label class="text-sm font-medium text-foreground">Quiz JSON</label>
          <textarea
            v-model="quizJson"
            class="font-mono text-xs w-full rounded-md border border-border bg-background p-3"
            :class="quizJsonValid ? '' : 'border-red-400'"
            rows="10"
          />
          <p v-if="!quizJsonValid" class="text-xs text-red-500">
            Invalid JSON — must have a <code>questions</code> array with
            <code>question</code>, <code>options</code>, and
            <code>correct_index</code> on each entry.
          </p>
        </div>
      </section>

      <!-- Error + submit -->
      <AppAlert v-if="error" type="error">{{ error }}</AppAlert>

      <div class="flex gap-3">
        <AppButton :loading="submitting" :disabled="!canSubmit" @click="submit">
          Publish Tutorial
        </AppButton>
        <AppButton variant="ghost" @click="router.back()">
          Cancel
        </AppButton>
      </div>
    </div>
  </div>
</template>
