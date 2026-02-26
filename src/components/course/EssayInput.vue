<script setup lang="ts">
import { ref, computed, onMounted, watch } from 'vue'
import { useLocalApi } from '@/composables/useLocalApi'
import { AppButton } from '@/components/ui'

interface EssayContent {
  question: string
  guidelines?: string
  min_words?: number
  max_words?: number
  rubric_criteria?: string[]
}

const props = defineProps<{
  contentCid: string | null
  contentInline?: string | null
  elementId: string
  isCompleted?: boolean
}>()

const emit = defineEmits<{
  (e: 'complete', score: number): void
}>()

const { invoke } = useLocalApi()
const essay = ref<EssayContent | null>(null)
const loading = ref(false)
const error = ref<string | null>(null)
const text = ref('')
const submitted = ref(false)
const saving = ref(false)
const lastSaved = ref<string | null>(null)
let saveTimer: ReturnType<typeof setTimeout> | null = null

const wordCount = computed(() => {
  const trimmed = text.value.trim()
  if (!trimmed) return 0
  return trimmed.split(/\s+/).length
})

const charCount = computed(() => text.value.length)

const minWords = computed(() => essay.value?.min_words ?? 0)
const maxWords = computed(() => essay.value?.max_words ?? Infinity)

const wordCountStatus = computed(() => {
  if (!essay.value) return 'neutral'
  if (minWords.value && wordCount.value < minWords.value) return 'short'
  if (maxWords.value && maxWords.value !== Infinity && wordCount.value > maxWords.value) return 'long'
  return 'valid'
})

const wordCountColor = computed(() => {
  switch (wordCountStatus.value) {
    case 'short': return 'text-amber-600 dark:text-amber-400'
    case 'long': return 'text-red-600 dark:text-red-400'
    case 'valid': return 'text-emerald-600 dark:text-emerald-400'
    default: return 'text-[rgb(var(--color-muted-foreground))]'
  }
})

const canSubmit = computed(() => {
  if (!text.value.trim()) return false
  if (wordCountStatus.value === 'long') return false
  if (minWords.value && wordCount.value < minWords.value) return false
  return true
})

const validationMessage = computed(() => {
  if (!essay.value) return null
  if (wordCountStatus.value === 'short') {
    const needed = minWords.value - wordCount.value
    return `Your response needs at least ${needed} more word${needed !== 1 ? 's' : ''}`
  }
  if (wordCountStatus.value === 'long') {
    const over = wordCount.value - maxWords.value
    return `Your response is ${over} word${over !== 1 ? 's' : ''} over the limit`
  }
  return null
})

async function loadContent() {
  // Prefer inline content (works on all platforms including mobile)
  if (props.contentInline) {
    try {
      essay.value = JSON.parse(props.contentInline) as EssayContent
      text.value = ''
      submitted.value = false
    } catch (e: unknown) {
      error.value = `Failed to parse essay prompt: ${e}`
      essay.value = null
    }
    return
  }
  if (!props.contentCid) { essay.value = null; return }
  loading.value = true
  error.value = null
  try {
    const bytes = await invoke<number[]>('content_resolve_bytes', { identifier: props.contentCid })
    const decoder = new TextDecoder()
    const json = decoder.decode(new Uint8Array(bytes))
    essay.value = JSON.parse(json) as EssayContent
    text.value = ''
    submitted.value = false
  } catch (e: unknown) {
    error.value = `Failed to load essay prompt: ${e}`
    essay.value = null
  } finally {
    loading.value = false
  }
}

function onInput() {
  if (saveTimer) clearTimeout(saveTimer)
  saveTimer = setTimeout(autoSave, 2000)
}

function autoSave() {
  if (!text.value.trim()) return
  saving.value = true
  // Save to localStorage as a simple draft mechanism
  try {
    localStorage.setItem(`essay_draft_${props.elementId}`, text.value)
    lastSaved.value = new Date().toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' })
  } catch { /* ignore */ }
  saving.value = false
}

function submitEssay() {
  if (!canSubmit.value) return
  submitted.value = true
  // Clean up draft
  try { localStorage.removeItem(`essay_draft_${props.elementId}`) } catch { /* ignore */ }
  emit('complete', 1) // Essay always scored as submitted (reviewed later)
}

// Restore draft on mount
function restoreDraft() {
  try {
    const draft = localStorage.getItem(`essay_draft_${props.elementId}`)
    if (draft) text.value = draft
  } catch { /* ignore */ }
}

onMounted(() => {
  loadContent()
  restoreDraft()
})

watch(() => props.contentCid, loadContent)
watch(() => props.elementId, () => {
  text.value = ''
  submitted.value = false
  restoreDraft()
  loadContent()
})
</script>

<template>
  <div class="essay-input">
    <!-- Loading -->
    <div v-if="loading" class="flex items-center justify-center py-12">
      <div class="h-8 w-8 animate-spin rounded-full border-2 border-[rgb(var(--color-primary))] border-t-transparent" />
    </div>

    <!-- Error -->
    <div v-else-if="error" class="rounded-lg border border-red-500/20 bg-red-500/10 p-4 text-sm text-red-600 dark:text-red-400">
      {{ error }}
    </div>

    <!-- No content -->
    <div v-else-if="!essay" class="py-8 text-center text-sm text-[rgb(var(--color-muted-foreground))]">
      No essay prompt available.
    </div>

    <!-- Essay Content -->
    <div v-else class="space-y-5">
      <!-- Type badge -->
      <span class="inline-flex rounded-full bg-purple-100 px-2.5 py-0.5 text-xs font-medium text-purple-700 dark:bg-purple-900/30 dark:text-purple-400">
        Written Response
      </span>

      <!-- Question -->
      <p class="text-base font-medium leading-relaxed text-[rgb(var(--color-foreground))]">
        {{ essay.question }}
      </p>

      <!-- Guidelines -->
      <div v-if="essay.guidelines" class="flex gap-3 rounded-lg bg-[rgb(var(--color-muted)/0.2)] p-4">
        <svg class="mt-0.5 h-4 w-4 flex-shrink-0 text-[rgb(var(--color-muted-foreground))]" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
          <path stroke-linecap="round" stroke-linejoin="round" d="M13 16h-1v-4h-1m1-4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
        </svg>
        <div>
          <p class="text-xs font-medium text-[rgb(var(--color-muted-foreground))]">Guidelines</p>
          <p class="mt-1 text-sm text-[rgb(var(--color-foreground))]">{{ essay.guidelines }}</p>
        </div>
      </div>

      <!-- Rubric criteria -->
      <div v-if="essay.rubric_criteria?.length" class="space-y-1.5">
        <p class="text-xs font-medium text-[rgb(var(--color-muted-foreground))]">Assessment criteria</p>
        <ul class="space-y-1">
          <li v-for="(criterion, idx) in essay.rubric_criteria" :key="idx" class="flex items-start gap-2 text-sm text-[rgb(var(--color-foreground))]">
            <svg class="mt-0.5 h-3.5 w-3.5 flex-shrink-0 text-[rgb(var(--color-primary))]" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
              <path stroke-linecap="round" stroke-linejoin="round" d="M5 13l4 4L19 7" />
            </svg>
            {{ criterion }}
          </li>
        </ul>
      </div>

      <!-- Textarea -->
      <div class="space-y-2">
        <textarea
          v-model="text"
          rows="12"
          :placeholder="submitted ? '' : 'Write your response here...'"
          :disabled="submitted || isCompleted"
          class="w-full resize-y rounded-lg border border-[rgb(var(--color-border))] bg-[rgb(var(--color-background))] px-4 py-3 text-sm text-[rgb(var(--color-foreground))] placeholder-[rgb(var(--color-muted-foreground)/0.5)] transition-colors focus:border-[rgb(var(--color-primary))] focus:outline-none focus:ring-1 focus:ring-[rgb(var(--color-primary))] disabled:opacity-60"
          @input="onInput"
        />

        <!-- Word/char count and save status -->
        <div class="flex items-center justify-between text-xs">
          <div class="flex items-center gap-4">
            <span :class="wordCountColor">
              {{ wordCount }} word{{ wordCount !== 1 ? 's' : '' }}
              <template v-if="minWords || (maxWords && maxWords !== Infinity)">
                <span class="text-[rgb(var(--color-muted-foreground))]">
                  ({{ minWords ? `min ${minWords}` : '' }}{{ minWords && maxWords && maxWords !== Infinity ? ', ' : '' }}{{ maxWords && maxWords !== Infinity ? `max ${maxWords}` : '' }})
                </span>
              </template>
            </span>
            <span class="text-[rgb(var(--color-muted-foreground))]">{{ charCount }} characters</span>
          </div>
          <span v-if="saving" class="text-[rgb(var(--color-muted-foreground))]">Saving...</span>
          <span v-else-if="lastSaved" class="text-[rgb(var(--color-muted-foreground))]">Saved at {{ lastSaved }}</span>
        </div>

        <!-- Validation message -->
        <p v-if="validationMessage" class="text-xs" :class="wordCountColor">
          {{ validationMessage }}
        </p>
      </div>

      <!-- Submitted banner -->
      <div v-if="submitted" class="flex items-center gap-3 rounded-lg bg-emerald-50 p-4 dark:bg-emerald-900/20">
        <svg class="h-5 w-5 text-emerald-600 dark:text-emerald-400" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
          <path stroke-linecap="round" stroke-linejoin="round" d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z" />
        </svg>
        <p class="text-sm text-emerald-800 dark:text-emerald-300">
          Your response has been submitted and will be reviewed.
        </p>
      </div>

      <!-- Submit button -->
      <div v-if="!submitted && !isCompleted" class="flex items-center gap-3">
        <AppButton
          :disabled="!canSubmit"
          @click="submitEssay"
        >
          Submit Response
        </AppButton>
      </div>
    </div>
  </div>
</template>
