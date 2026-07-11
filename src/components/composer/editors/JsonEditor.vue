<script setup lang="ts">
// Generic inline-JSON editor for interactive/assessment elements whose
// content shape is defined by the player component.
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useLocalApi } from '@/composables/useLocalApi'
import { AppButton } from '@/components/ui'
import type { Element } from '@/types'

const props = defineProps<{ element: Element }>()
const emit = defineEmits<{ updated: [Element] }>()

const { invoke } = useLocalApi()
const { t } = useI18n()

const body = ref(props.element.content_inline ?? '')
const dirty = ref(false)
const saving = ref(false)
const error = ref('')

watch(() => props.element.id, () => {
  body.value = props.element.content_inline ?? ''
  dirty.value = false
})

const jsonValid = computed(() => {
  if (!body.value.trim()) return true
  try {
    JSON.parse(body.value)
    return true
  } catch {
    return false
  }
})

async function save() {
  if (!jsonValid.value) {
    error.value = t('instructor.editors.json.errFixJson')
    return
  }
  saving.value = true
  error.value = ''
  try {
    const updated = await invoke<Element>('update_element', {
      elementId: props.element.id,
      req: { content_inline: body.value },
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
  <div class="space-y-3">
    <div class="flex items-center justify-between">
      <h3 class="text-sm font-semibold text-foreground">
        {{ element.element_type === 'assessment' ? $t('instructor.editors.json.assessmentHeading') : $t('instructor.editors.json.interactiveHeading') }}
      </h3>
      <AppButton v-if="dirty" size="xs" :loading="saving" :disabled="!jsonValid" @click="save">{{ $t('common.actions.save') }}</AppButton>
    </div>
    <textarea
      v-model="body"
      rows="16"
      class="w-full rounded-md border bg-background p-3 font-mono text-xs"
      :class="jsonValid ? 'border-border' : 'border-error'"
      placeholder='{ "questions": [...] }'
      @input="dirty = true"
    />
    <p v-if="!jsonValid" class="text-xs text-error">{{ $t('instructor.editors.json.invalidJson') }}</p>
    <p v-if="error" class="text-sm text-error">{{ error }}</p>
  </div>
</template>
