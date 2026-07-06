<script setup lang="ts">
// Essay editor — prompt + grading rubric, stored as content_inline JSON.
// Essay submissions are manually reviewed, feeding the instructor inbox.
import { ref, watch } from 'vue'
import { useLocalApi } from '@/composables/useLocalApi'
import { AppButton } from '@/components/ui'
import type { Element } from '@/types'

const props = defineProps<{ element: Element }>()
const emit = defineEmits<{ updated: [Element] }>()

const { invoke } = useLocalApi()

function parse(): { prompt: string; rubric: string; min_words: number | null } {
  try {
    const p = JSON.parse(props.element.content_inline ?? '')
    return {
      prompt: typeof p.prompt === 'string' ? p.prompt : '',
      rubric: typeof p.rubric === 'string' ? p.rubric : '',
      min_words: typeof p.min_words === 'number' ? p.min_words : null,
    }
  } catch {
    return { prompt: '', rubric: '', min_words: null }
  }
}

const initial = parse()
const prompt = ref(initial.prompt)
const rubric = ref(initial.rubric)
const minWords = ref<number | null>(initial.min_words)
const dirty = ref(false)
const saving = ref(false)
const error = ref('')

watch(() => props.element.id, () => {
  const next = parse()
  prompt.value = next.prompt
  rubric.value = next.rubric
  minWords.value = next.min_words
  dirty.value = false
})

async function save() {
  if (!prompt.value.trim()) {
    error.value = 'The essay needs a prompt.'
    return
  }
  saving.value = true
  error.value = ''
  try {
    const updated = await invoke<Element>('update_element', {
      elementId: props.element.id,
      req: {
        content_inline: JSON.stringify({
          prompt: prompt.value.trim(),
          rubric: rubric.value.trim(),
          min_words: minWords.value,
        }),
      },
    })
    emit('updated', updated)
    dirty.value = false
  } catch (e) {
    error.value = String(e)
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <div class="space-y-4">
    <div class="flex items-center justify-between">
      <h3 class="text-sm font-semibold text-foreground">Essay assignment</h3>
      <AppButton v-if="dirty" size="xs" :loading="saving" @click="save">Save</AppButton>
    </div>

    <div>
      <label class="mb-1 block text-xs font-medium text-muted-foreground">Prompt</label>
      <textarea
        v-model="prompt"
        rows="5"
        class="w-full rounded-md border border-border bg-background px-3 py-2 text-sm"
        placeholder="What should the learner write about?"
        @input="dirty = true"
      />
    </div>

    <div>
      <label class="mb-1 block text-xs font-medium text-muted-foreground">
        Grading rubric (shown to you when reviewing, and to the learner up front)
      </label>
      <textarea
        v-model="rubric"
        rows="5"
        class="w-full rounded-md border border-border bg-background px-3 py-2 text-sm"
        placeholder="e.g., Thesis clarity (30%), Evidence (40%), Structure (30%)"
        @input="dirty = true"
      />
    </div>

    <div>
      <label class="mb-1 block text-xs font-medium text-muted-foreground">Minimum words (optional)</label>
      <input
        v-model.number="minWords"
        type="number"
        min="0"
        class="w-32 rounded-md border border-border bg-background px-3 py-2 text-sm"
        @input="dirty = true"
      >
    </div>

    <p class="text-xs text-muted-foreground">
      Essay submissions land in your instructor inbox for manual review and scoring.
    </p>
    <p v-if="error" class="text-sm text-error">{{ error }}</p>
  </div>
</template>
