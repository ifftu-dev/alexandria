<script setup lang="ts">
// Course outline: chapter → element tree with add + drag reorder.
// For tutorials (one implicit chapter) the chapter level is hidden.
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { useLocalApi } from '@/composables/useLocalApi'
import { AppButton } from '@/components/ui'
import { useDragReorder } from './useDragReorder'
import type { Chapter, CreateChapterRequest, CreateElementRequest, Element } from '@/types'

const props = defineProps<{
  courseId: string
  chapters: Chapter[]
  elements: Record<string, Element[]>
  selectedElementId: string | null
  /** Tutorial mode hides chapter chrome (single implicit chapter). */
  flat: boolean
}>()

const emit = defineEmits<{
  select: [Element]
  changed: [] // structural change → parent reloads outline
}>()

const { invoke } = useLocalApi()
const { t } = useI18n()

const error = ref('')

// ── Add chapter ─────────────────────────────────────────────────
const showNewChapter = ref(false)
const newChapterTitle = ref('')
const creatingChapter = ref(false)

async function addChapter() {
  if (!newChapterTitle.value.trim()) return
  creatingChapter.value = true
  try {
    const req: CreateChapterRequest = { title: newChapterTitle.value.trim() }
    await invoke('create_chapter', { courseId: props.courseId, req })
    newChapterTitle.value = ''
    showNewChapter.value = false
    emit('changed')
  } catch (e) {
    error.value = String(e)
  } finally {
    creatingChapter.value = false
  }
}

// ── Add element ─────────────────────────────────────────────────
const ELEMENT_TYPES = computed(() => [
  { value: 'video', label: t('instructor.elementTypes.video') },
  { value: 'text', label: t('instructor.elementTypes.text') },
  { value: 'pdf', label: t('instructor.elementTypes.pdf') },
  { value: 'downloadable', label: t('instructor.elementTypes.downloadable') },
  { value: 'quiz', label: t('instructor.elementTypes.quiz') },
  { value: 'objective_single_mcq', label: t('instructor.elementTypes.objectiveSingleMcq') },
  { value: 'objective_multi_mcq', label: t('instructor.elementTypes.objectiveMultiMcq') },
  { value: 'subjective_mcq', label: t('instructor.elementTypes.subjectiveMcq') },
  { value: 'essay', label: t('instructor.elementTypes.essay') },
  { value: 'interactive', label: t('instructor.elementTypes.interactive') },
  { value: 'assessment', label: t('instructor.elementTypes.assessment') },
  { value: 'plugin', label: t('instructor.elementTypes.plugin') },
])

const addingToChapter = ref<string | null>(null)
const newElementTitle = ref('')
const newElementType = ref('video')
const creatingElement = ref(false)

async function addElement(chapterId: string) {
  if (!newElementTitle.value.trim()) return
  creatingElement.value = true
  try {
    // Plugin elements are created unbound; the plugin editor sets the
    // cid/config before the element is playable.
    const req: CreateElementRequest = {
      title: newElementTitle.value.trim(),
      element_type: newElementType.value,
    }
    const el = await invoke<Element>('create_element', { chapterId, req })
    newElementTitle.value = ''
    addingToChapter.value = null
    emit('changed')
    emit('select', el)
  } catch (e) {
    error.value = String(e)
  } finally {
    creatingElement.value = false
  }
}

// ── Drag reorder ────────────────────────────────────────────────
const chapterDrag = useDragReorder(async (ids) => {
  try {
    await invoke('reorder_chapters', { courseId: props.courseId, orderedIds: ids })
    emit('changed')
  } catch (e) {
    error.value = String(e)
  }
})

// One reorder handler per chapter; the chapter id travels with the drop.
function elementDragFor(chapterId: string) {
  return useDragReorder(async (ids) => {
    try {
      await invoke('reorder_elements', { chapterId, orderedIds: ids })
      emit('changed')
    } catch (e) {
      error.value = String(e)
    }
  })
}

// Cache per chapter so drag state survives re-renders.
const elementDrags = new Map<string, ReturnType<typeof useDragReorder>>()
function dragFor(chapterId: string) {
  let d = elementDrags.get(chapterId)
  if (!d) {
    d = elementDragFor(chapterId)
    elementDrags.set(chapterId, d)
  }
  return d
}

function elementIds(chapterId: string): string[] {
  return (props.elements[chapterId] ?? []).map(e => e.id)
}

function typeLabel(type: string): string {
  return ELEMENT_TYPES.value.find(e => e.value === type)?.label ?? type
}
</script>

<template>
  <div class="space-y-3">
    <div
      v-for="(chapter, chIdx) in chapters"
      :key="chapter.id"
      class="rounded-lg border border-border bg-card"
      :class="{ 'opacity-50': chapterDrag.draggingId.value === chapter.id }"
    >
      <!-- Chapter header (hidden in flat/tutorial mode) -->
      <div
        v-if="!flat"
        class="flex items-center gap-2 px-3 py-2 border-b border-border/60 cursor-grab"
        draggable="true"
        @dragstart="chapterDrag.onDragStart(chapter.id, $event)"
        @dragover="chapterDrag.onDragOver(chapter.id, $event)"
        @drop="chapterDrag.onDrop(chapter.id, chapters.map(c => c.id))"
        @dragend="chapterDrag.onDragEnd"
      >
        <svg class="h-3.5 w-3.5 text-muted-foreground/60 shrink-0" viewBox="0 0 20 20" fill="currentColor">
          <path d="M7 4a1 1 0 110 2 1 1 0 010-2zm0 5a1 1 0 110 2 1 1 0 010-2zm0 5a1 1 0 110 2 1 1 0 010-2zm6-10a1 1 0 110 2 1 1 0 010-2zm0 5a1 1 0 110 2 1 1 0 010-2zm0 5a1 1 0 110 2 1 1 0 010-2z" />
        </svg>
        <span class="flex h-5 w-5 items-center justify-center rounded bg-primary/10 text-[10px] font-bold text-primary shrink-0">
          {{ chIdx + 1 }}
        </span>
        <span class="text-sm font-medium text-foreground truncate">{{ chapter.title }}</span>
        <span class="ml-auto text-xs text-muted-foreground shrink-0">
          {{ (elements[chapter.id] ?? []).length }}
        </span>
      </div>

      <!-- Elements -->
      <div class="p-1.5 space-y-0.5">
        <button
          v-for="el in elements[chapter.id] ?? []"
          :key="el.id"
          class="w-full flex items-center gap-2 rounded-md px-2.5 py-2 text-left text-sm transition-colors cursor-grab"
          :class="[
            selectedElementId === el.id ? 'bg-primary/10 text-primary' : 'text-foreground hover:bg-muted/40',
            { 'opacity-50': dragFor(chapter.id).draggingId.value === el.id },
          ]"
          draggable="true"
          @click="emit('select', el)"
          @dragstart="dragFor(chapter.id).onDragStart(el.id, $event)"
          @dragover="dragFor(chapter.id).onDragOver(el.id, $event)"
          @drop="dragFor(chapter.id).onDrop(el.id, elementIds(chapter.id))"
          @dragend="dragFor(chapter.id).onDragEnd"
        >
          <span class="truncate">{{ el.title }}</span>
          <span class="ml-auto text-[10px] uppercase tracking-wide text-muted-foreground shrink-0">
            {{ typeLabel(el.element_type) }}
          </span>
        </button>

        <!-- Add element -->
        <div v-if="addingToChapter === chapter.id" class="rounded-md border border-dashed border-border p-2 space-y-2">
          <input
            v-model="newElementTitle"
            class="w-full rounded-md border border-border bg-background px-2 py-1.5 text-sm"
            :placeholder="$t('instructor.outline.elementTitlePlaceholder')"
            @keydown.enter="addElement(chapter.id)"
          >
          <select
            v-model="newElementType"
            class="w-full rounded-md border border-border bg-background px-2 py-1.5 text-sm"
          >
            <option v-for="t in ELEMENT_TYPES" :key="t.value" :value="t.value">{{ t.label }}</option>
          </select>
          <div class="flex gap-1.5">
            <AppButton size="xs" :loading="creatingElement" @click="addElement(chapter.id)">{{ $t('instructor.outline.add') }}</AppButton>
            <AppButton variant="ghost" size="xs" @click="addingToChapter = null">{{ $t('common.actions.cancel') }}</AppButton>
          </div>
        </div>
        <button
          v-else
          class="w-full rounded-md px-2.5 py-1.5 text-left text-xs text-muted-foreground hover:text-primary hover:bg-muted/30 transition-colors"
          @click="addingToChapter = chapter.id; newElementTitle = ''"
        >
          {{ $t('instructor.outline.addElement') }}
        </button>
      </div>
    </div>

    <!-- Add chapter (course mode only) -->
    <template v-if="!flat">
      <div v-if="showNewChapter" class="rounded-lg border border-dashed border-border bg-card p-3 space-y-2">
        <input
          v-model="newChapterTitle"
          class="w-full rounded-md border border-border bg-background px-2 py-1.5 text-sm"
          :placeholder="$t('instructor.outline.chapterTitlePlaceholder')"
          @keydown.enter="addChapter"
        >
        <div class="flex gap-1.5">
          <AppButton size="xs" :loading="creatingChapter" @click="addChapter">{{ $t('instructor.outline.addChapter') }}</AppButton>
          <AppButton variant="ghost" size="xs" @click="showNewChapter = false">{{ $t('common.actions.cancel') }}</AppButton>
        </div>
      </div>
      <AppButton v-else variant="outline" size="sm" class="w-full" @click="showNewChapter = true">
        {{ $t('instructor.outline.addChapterCta') }}
      </AppButton>
    </template>

    <p v-if="error" class="text-xs text-error">{{ error }}</p>
  </div>
</template>
