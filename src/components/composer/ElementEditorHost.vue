<script setup lang="ts">
// Hosts the per-type editor plus the fields every element shares:
// title, duration, and skill tags.
import { computed, ref, watch, type Component } from 'vue'
import { useLocalApi } from '@/composables/useLocalApi'
import { AppButton, StatusBadge, ConfirmDialog } from '@/components/ui'
import SkillTagPicker from './SkillTagPicker.vue'
import VideoEditor from './editors/VideoEditor.vue'
import TextEditor from './editors/TextEditor.vue'
import FileEditor from './editors/FileEditor.vue'
import QuizEditor from './editors/QuizEditor.vue'
import EssayEditor from './editors/EssayEditor.vue'
import JsonEditor from './editors/JsonEditor.vue'
import PluginEditor from './editors/PluginEditor.vue'
import type { Element } from '@/types'

const props = defineProps<{ element: Element }>()
const emit = defineEmits<{ updated: [Element]; deleted: [string] }>()

const { invoke } = useLocalApi()

const editorFor = computed<Component>(() => {
  switch (props.element.element_type) {
    case 'video': return VideoEditor
    case 'text': return TextEditor
    case 'pdf':
    case 'downloadable': return FileEditor
    case 'quiz':
    case 'objective_single_mcq':
    case 'objective_multi_mcq':
    case 'subjective_mcq': return QuizEditor
    case 'essay': return EssayEditor
    case 'plugin': return PluginEditor
    default: return JsonEditor // interactive, assessment, future types
  }
})

const title = ref(props.element.title)
const titleDirty = ref(false)
const savingTitle = ref(false)
const showDelete = ref(false)
const deleting = ref(false)
const error = ref('')

watch(() => props.element.id, () => {
  title.value = props.element.title
  titleDirty.value = false
  error.value = ''
})

async function saveTitle() {
  if (!title.value.trim()) return
  savingTitle.value = true
  try {
    const updated = await invoke<Element>('update_element', {
      elementId: props.element.id,
      req: { title: title.value.trim() },
    })
    emit('updated', updated)
    titleDirty.value = false
  } catch (e) {
    error.value = String(e)
  } finally {
    savingTitle.value = false
  }
}

async function deleteElement() {
  deleting.value = true
  try {
    await invoke('delete_element', { elementId: props.element.id })
    emit('deleted', props.element.id)
  } catch (e) {
    error.value = String(e)
  } finally {
    deleting.value = false
    showDelete.value = false
  }
}
</script>

<template>
  <div class="space-y-6">
    <!-- Shared header -->
    <div class="space-y-3">
      <div class="flex items-center gap-2">
        <StatusBadge :status="element.element_type" />
        <span class="text-xs text-muted-foreground font-mono truncate">{{ element.id.slice(0, 12) }}…</span>
        <AppButton variant="danger" size="xs" class="ml-auto" @click="showDelete = true">
          Delete element
        </AppButton>
      </div>
      <div class="flex items-center gap-2">
        <input
          v-model="title"
          type="text"
          class="flex-1 rounded-md border border-border bg-background px-3 py-2 text-base font-semibold"
          placeholder="Element title"
          @input="titleDirty = true"
          @keydown.enter="saveTitle"
        >
        <AppButton v-if="titleDirty" size="sm" :loading="savingTitle" @click="saveTitle">Save</AppButton>
      </div>
      <SkillTagPicker :key="element.id" :element-id="element.id" />
    </div>

    <div class="border-t border-border pt-5">
      <component
        :is="editorFor"
        :key="element.id"
        :element="element"
        @updated="(el: Element) => emit('updated', el)"
      />
    </div>

    <p v-if="error" class="text-sm text-error">{{ error }}</p>

    <ConfirmDialog
      :open="showDelete"
      title="Delete Element"
      message="This removes the element and its content from the course. This cannot be undone."
      confirm-label="Delete"
      confirm-variant="danger"
      :loading="deleting"
      @confirm="deleteElement"
      @cancel="showDelete = false"
    />
  </div>
</template>
