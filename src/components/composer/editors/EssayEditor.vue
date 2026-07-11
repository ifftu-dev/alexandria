<script setup lang="ts">
// Essay editor — prompt + grading rubric, stored as content_inline JSON.
// Essay submissions are manually reviewed, feeding the instructor inbox.
import { ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useLocalApi } from '@/composables/useLocalApi'
import { AppButton } from '@/components/ui'
import type { Element } from '@/types'

const props = defineProps<{ element: Element }>()
const emit = defineEmits<{ updated: [Element] }>()

const { invoke } = useLocalApi()
const { t } = useI18n()

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
    error.value = t('instructor.editors.essay.errNeedPrompt')
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
      <h3 class="text-sm font-semibold text-foreground">{{ $t('instructor.editors.essay.heading') }}</h3>
      <AppButton v-if="dirty" size="xs" :loading="saving" @click="save">{{ $t('common.actions.save') }}</AppButton>
    </div>

    <div>
      <label class="mb-1 block text-xs font-medium text-muted-foreground">{{ $t('instructor.editors.essay.promptLabel') }}</label>
      <textarea
        v-model="prompt"
        rows="5"
        class="w-full rounded-md border border-border bg-background px-3 py-2 text-sm"
        :placeholder="$t('instructor.editors.essay.promptPlaceholder')"
        @input="dirty = true"
      />
    </div>

    <div>
      <label class="mb-1 block text-xs font-medium text-muted-foreground">
        {{ $t('instructor.editors.essay.rubricLabel') }}
      </label>
      <textarea
        v-model="rubric"
        rows="5"
        class="w-full rounded-md border border-border bg-background px-3 py-2 text-sm"
        :placeholder="$t('instructor.editors.essay.rubricPlaceholder')"
        @input="dirty = true"
      />
    </div>

    <div>
      <label class="mb-1 block text-xs font-medium text-muted-foreground">{{ $t('instructor.editors.essay.minWordsLabel') }}</label>
      <input
        v-model.number="minWords"
        type="number"
        min="0"
        class="w-32 rounded-md border border-border bg-background px-3 py-2 text-sm"
        @input="dirty = true"
      >
    </div>

    <p class="text-xs text-muted-foreground">
      {{ $t('instructor.editors.essay.hint') }}
    </p>
    <p v-if="error" class="text-sm text-error">{{ error }}</p>
  </div>
</template>
