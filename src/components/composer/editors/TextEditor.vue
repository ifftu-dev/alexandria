<script setup lang="ts">
// Text lesson editor — markdown body stored in content_inline.
import { ref, watch } from 'vue'
import { useLocalApi } from '@/composables/useLocalApi'
import { AppButton } from '@/components/ui'
import type { Element } from '@/types'

const props = defineProps<{ element: Element }>()
const emit = defineEmits<{ updated: [Element] }>()

const { invoke } = useLocalApi()

const body = ref(props.element.content_inline ?? '')
const dirty = ref(false)
const saving = ref(false)
const error = ref('')

watch(() => props.element.id, () => {
  body.value = props.element.content_inline ?? ''
  dirty.value = false
})

async function save() {
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
      <h3 class="text-sm font-semibold text-foreground">{{ $t('instructor.editors.text.heading') }}</h3>
      <AppButton v-if="dirty" size="xs" :loading="saving" @click="save">{{ $t('common.actions.save') }}</AppButton>
    </div>
    <textarea
      v-model="body"
      rows="18"
      class="w-full rounded-md border border-border bg-background p-3 font-mono text-sm"
      :placeholder="$t('instructor.editors.text.placeholder')"
      @input="dirty = true"
    />
    <p v-if="error" class="text-sm text-error">{{ error }}</p>
  </div>
</template>
