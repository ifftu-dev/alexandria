<script setup lang="ts">
// Unified course & tutorial composer.
//
// One surface for both kinds: outline on the left (chapters hidden for
// tutorials — one implicit chapter), the selected element's editor on
// the right. Draft ↔ publish lifecycle wraps the existing
// `publish_course` pipeline.
import { computed, ref, watch } from 'vue'
import { onBeforeRouteLeave, useRoute, useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'
import { useLocalApi } from '@/composables/useLocalApi'
import { AppButton, AppInput, AppTabs, AppTextarea, ConfirmDialog, EmptyState, StatusBadge } from '@/components/ui'
import OutlinePanel from '@/components/composer/OutlinePanel.vue'
import ElementEditorHost from '@/components/composer/ElementEditorHost.vue'
import StudioRunPanel from '@/components/composer/StudioRunPanel.vue'
import { useStudio } from '@/composables/useStudio'
import type { CourseStudio, LessonFeedback, StudioDocument, StudioRun, TutorPolicy } from '@/types'
import type {
  Chapter,
  Course,
  CreateCourseRequest,
  Element,
  PublishCourseResult,
  UpdateCourseRequest,
} from '@/types'

const { invoke } = useLocalApi()
const route = useRoute()
const router = useRouter()
const { t } = useI18n()
const studio = useStudio()
const stage = ref('curriculum')
const courseStudio = ref<StudioDocument<CourseStudio> | null>(null)
const studioBaseline = ref('')
const studioDirty = computed(() => courseStudio.value !== null && JSON.stringify(courseStudio.value) !== studioBaseline.value)
const savingStudio = ref(false)
const editorDirty = ref(false)
const editorRevision = ref(0)
const discardEditor = ref(false)
let editorDecision: ((leave: boolean) => void) | null = null
function confirmEditorLeave(): Promise<boolean> {
  if (!editorDirty.value) return Promise.resolve(true)
  discardEditor.value = true
  return new Promise(resolve => { editorDecision = resolve })
}
function resolveEditorLeave(leave: boolean) { discardEditor.value = false; if (leave) { editorDirty.value = false; editorRevision.value++ }; editorDecision?.(leave); editorDecision = null }
async function selectElement(element: Element) { if (await confirmEditorLeave()) selectedElement.value = element }

const selectedWorkflow = ref('')
const initialPrompt = ref('')
const showRunSetup = ref(false)
const activeRun = ref<string | null>(null)
const preparingRun = ref(false)
const lessonFeedback = ref<LessonFeedback[]>([])
const stages = computed(() => [
  { id: 'brief', label: t('instructor.studio.brief'), done: Boolean(courseStudio.value?.value.outcome.trim()), status: courseStudio.value?.value.outcome.trim() ? t('instructor.studio.saved') : t('instructor.studio.notStarted') },
  { id: 'sources', label: t('instructor.studio.sources'), done: Boolean(courseStudio.value?.value.sources.some(source => source.selected)), status: t('instructor.studio.sourceCount', { count: courseStudio.value?.value.sources.filter(source => source.selected).length ?? 0 }) },
  { id: 'curriculum', label: t('instructor.studio.curriculum'), done: false, status: t('instructor.studio.elementCount', { count: totalElements.value }) },
  { id: 'review', label: t('instructor.studio.review'), done: course.value?.status === 'published', status: t(course.value?.status === 'published' ? 'instructor.studio.published' : 'instructor.studio.awaitingReview') },
])
const stageIndex = computed(() => stages.value.findIndex(item => item.id === stage.value))
async function saveStudio() {
  if (!courseStudio.value || !studioDirty.value) return true
  savingStudio.value = true; error.value = ''
  try {
    courseStudio.value = await invoke<StudioDocument<CourseStudio>>('studio_save_course', { document: courseStudio.value })
    studioBaseline.value = JSON.stringify(courseStudio.value)
    return true
  } catch (e) { error.value = String(e); return false } finally { savingStudio.value = false }
}
async function switchStage(next: string) { if (next === stage.value) return; if (!await confirmEditorLeave()) return; if (await saveStudio()) { stage.value = next; showRunSetup.value = false } }
onBeforeRouteLeave(async () => await confirmEditorLeave() && (!studioDirty.value || await saveStudio()))
function addSource() { courseStudio.value?.value.sources.push({ id: crypto.randomUUID(), title: '', text: '', selected: true }) }
async function openRunSetup() {
  if (editorDirty.value) { error.value = t('instructor.studio.saveLessonFirst'); return }
  try { await studio.refresh(); showRunSetup.value = true; selectedWorkflow.value ||= studio.workflows.value[0]?.id ?? '' } catch (e) { error.value = String(e) }
}
async function prepareRun() {
  if (!course.value || !selectedElement.value || !await saveStudio()) return
  preparingRun.value = true; error.value = ''
  try {
    const run = await invoke<StudioDocument<StudioRun>>('studio_prepare_run', { courseId: course.value.id, elementId: selectedElement.value.id, workflowId: selectedWorkflow.value, initialPrompt: initialPrompt.value })
    activeRun.value = run.id; showRunSetup.value = false
  } catch (e) { error.value = String(e) } finally { preparingRun.value = false }
}

async function improveFromFeedback(item: LessonFeedback) {
  const element = Object.values(elements.value).flat().find(value => value.id === item.element_id)
  if (!element) return
  selectedElement.value = element
  initialPrompt.value = t('instructor.studio.improvePrompt')
  await switchStage('curriculum')
  await openRunSetup()
}


const isNew = computed(() => route.params.id === undefined)
const kind = computed<'course' | 'tutorial'>(() => {
  if (course.value) return course.value.kind === 'tutorial' ? 'tutorial' : 'course'
  return route.query.kind === 'tutorial' ? 'tutorial' : 'course'
})

const course = ref<Course | null>(null)
const chapters = ref<Chapter[]>([])
const elements = ref<Record<string, Element[]>>({})
const selectedElement = ref<Element | null>(null)
const workflowSupported = computed(() => Boolean(selectedElement.value && [
  'text', 'quiz', 'assessment', 'objective_single_mcq', 'objective_multi_mcq',
  'subjective_mcq', 'essay',
].includes(selectedElement.value.element_type)))
const loading = ref(false)
const error = ref('')

// ── New draft form ──────────────────────────────────────────────
const newTitle = ref('')
const newDescription = ref('')
const newTagsCsv = ref('')
const creating = ref(false)

async function createDraft() {
  if (!newTitle.value.trim()) {
    error.value = t('instructor.compose.needTitle')
    return
  }
  creating.value = true
  error.value = ''
  try {
    const req: CreateCourseRequest = {
      title: newTitle.value.trim(),
      description: newDescription.value.trim() || null,
      tags: newTagsCsv.value.split(',').map(t => t.trim()).filter(Boolean),
      kind: kind.value,
    }
    const created = await invoke<Course>('create_course', { req })
    // Tutorials get their single implicit chapter up front.
    if (kind.value === 'tutorial') {
      await invoke('create_chapter', {
        courseId: created.id,
        req: { title: 'Content' },
      })
    }
    await router.replace(`/instructor/composer/${created.id}`)
  } catch (e) {
    error.value = String(e)
  } finally {
    creating.value = false
  }
}

// ── Load ────────────────────────────────────────────────────────
async function load(courseId: string) {
  loading.value = true
  error.value = ''
  try {
    course.value = await invoke<Course>('get_course', { courseId })
    if (!course.value) throw new Error(t('instructor.compose.notFoundTitle'))
    courseStudio.value = await invoke<StudioDocument<CourseStudio>>('studio_get_course', { courseId })
    studioBaseline.value = JSON.stringify(courseStudio.value)
    await reloadOutline()
    lessonFeedback.value = await invoke<LessonFeedback[]>('studio_list_lesson_feedback', { courseId, elementId: null }).catch(() => [])
  } catch (e) {
    error.value = String(e)
    course.value = null
  } finally {
    loading.value = false
  }
}

async function reloadAfterRun() {
  await reloadOutline()
  editorRevision.value++
  editorDirty.value = false
}

async function reloadOutline() {
  if (!course.value) return
  chapters.value = await invoke<Chapter[]>('list_chapters', { courseId: course.value.id })
  const map: Record<string, Element[]> = {}
  for (const ch of chapters.value) {
    map[ch.id] = await invoke<Element[]>('list_elements', { chapterId: ch.id })
  }
  elements.value = map
  if (!selectedElement.value) selectedElement.value = Object.values(map).flat()[0] ?? null
  // Keep the selection fresh (it may have moved or been deleted).
  if (selectedElement.value) {
    const found = Object.values(map).flat().find(e => e.id === selectedElement.value?.id)
    selectedElement.value = found ?? null
  }
}

watch(() => route.params.id, (id) => {
  selectedElement.value = null
  activeRun.value = null
  editorDirty.value = false
  if (typeof id === 'string') void load(id)
}, { immediate: true })

// ── Course metadata editing ─────────────────────────────────────
const editingMeta = ref(false)
const metaTab = ref('details')
const metaTutor = ref<TutorPolicy>({ enabled: false, guidance: 'socratic', initial_prompt: '' })
const metaTitle = ref('')
const metaDescription = ref('')
const savingMeta = ref(false)

function openMetaEditor() {
  if (!course.value) return
  metaTitle.value = course.value.title
  metaDescription.value = course.value.description ?? ''
  if (courseStudio.value) metaTutor.value = structuredClone(courseStudio.value.value.tutor)
  metaTab.value = 'details'
  editingMeta.value = true
}

function closeMetaEditor() {
  editingMeta.value = false
}

async function saveMeta() {
  if (!course.value || !metaTitle.value.trim()) return
  savingMeta.value = true
  try {
    const req: UpdateCourseRequest = {
      title: metaTitle.value.trim(),
      description: metaDescription.value.trim() || null,
    }
    course.value = await invoke<Course>('update_course', { courseId: course.value.id, req })
    if (courseStudio.value) courseStudio.value.value.tutor = structuredClone(metaTutor.value)
    if (await saveStudio()) editingMeta.value = false
  } catch (e) {
    error.value = String(e)
  } finally {
    savingMeta.value = false
  }
}

// ── Element selection + mutation fan-in ─────────────────────────
function onElementUpdated(el: Element) {
  selectedElement.value = el
  const list = elements.value[el.chapter_id]
  if (list) {
    const idx = list.findIndex(e => e.id === el.id)
    if (idx >= 0) list[idx] = el
  }
}

function onElementDeleted(id: string) {
  if (selectedElement.value?.id === id) selectedElement.value = null
  void reloadOutline()
}

// ── Publish / delete ────────────────────────────────────────────
const showPublishConfirm = ref(false)
const releaseReviewed = ref(false)
watch([courseStudio, elements, course], () => { releaseReviewed.value = false }, { deep: true })
const publishing = ref(false)
const publishResult = ref<PublishCourseResult | null>(null)
const showDeleteConfirm = ref(false)
const deleting = ref(false)

const totalElements = computed(() =>
  Object.values(elements.value).reduce((n, list) => n + list.length, 0),
)

const publishBlockers = computed<string[]>(() => {
  const blockers: string[] = []
  if (!totalElements.value) blockers.push(t('instructor.compose.blockerNeedElement'))
  const unbound = Object.values(elements.value)
    .flat()
    .filter(e => e.element_type === 'plugin' && !e.plugin_cid)
  if (unbound.length) {
    blockers.push(t('instructor.compose.blockerUnboundPlugins', { count: unbound.length }, unbound.length))
  }
  return blockers
})

async function publish() {
  if (!course.value) return
  publishing.value = true
  error.value = ''
  try {
    publishResult.value = await invoke<PublishCourseResult>('publish_course', {
      courseId: course.value.id,
    })
    course.value = await invoke<Course>('get_course', { courseId: course.value.id })
  } catch (e) {
    error.value = String(e)
  } finally {
    publishing.value = false
    showPublishConfirm.value = false
  }
}

async function deleteCourse() {
  if (!course.value) return
  deleting.value = true
  try {
    await invoke('delete_course', { courseId: course.value.id })
    router.replace('/instructor')
  } catch (e) {
    error.value = String(e)
  } finally {
    deleting.value = false
    showDeleteConfirm.value = false
  }
}
</script>

<template>
  <!-- ── New draft ─────────────────────────────────────────────── -->
  <div v-if="isNew && !course" class="max-w-2xl">
    <div class="mb-8">
      <h1 class="text-3xl font-bold text-foreground">
        {{ kind === 'tutorial' ? $t('instructor.compose.newTutorial') : $t('instructor.compose.newCourse') }}
      </h1>
      <p class="mt-2 text-muted-foreground">
        {{ kind === 'tutorial'
          ? $t('instructor.compose.tutorialIntro')
          : $t('instructor.compose.courseIntro') }}
      </p>
    </div>

    <div class="rounded-xl border border-border bg-card p-6 space-y-5">
      <AppInput v-model="newTitle" :label="$t('instructor.compose.titleLabel')" :placeholder="$t('instructor.compose.titlePlaceholder')" />
      <AppTextarea v-model="newDescription" :label="$t('instructor.compose.descriptionLabel')" :rows="3" :placeholder="$t('instructor.compose.descriptionPlaceholder')" />
      <AppInput v-model="newTagsCsv" :label="$t('instructor.compose.tagsLabel')" :placeholder="$t('instructor.compose.tagsPlaceholder')" />
      <p v-if="error" class="text-sm text-error">{{ error }}</p>
      <div class="flex gap-3">
        <AppButton type="button" :loading="creating" @click="createDraft">{{ $t('instructor.compose.createDraft') }}</AppButton>
        <AppButton type="button" variant="ghost" @click="router.back()">{{ $t('common.actions.cancel') }}</AppButton>
      </div>
    </div>
  </div>

  <!-- ── Loading ───────────────────────────────────────────────── -->
  <div v-else-if="loading" class="space-y-4">
    <div class="h-8 w-72 animate-pulse rounded bg-muted-foreground/15" />
    <div class="grid grid-cols-[280px_1fr] gap-4">
      <div class="h-96 animate-pulse rounded-xl bg-muted-foreground/8" />
      <div class="h-96 animate-pulse rounded-xl bg-muted-foreground/8" />
    </div>
  </div>

  <!-- ── Composer ──────────────────────────────────────────────── -->
  <div v-else-if="course" class="space-y-5 min-w-0">
    <!-- Header -->
    <div class="flex flex-wrap items-start justify-between gap-4">
      <div class="min-w-0">
        <div class="flex items-center gap-2 mb-1.5">
          <StatusBadge :status="course.status" />
          <span class="rounded-full bg-muted px-2 py-0.5 text-[10px] font-medium uppercase tracking-wide text-muted-foreground">
            {{ kind }}
          </span>
          <span class="text-xs text-muted-foreground">v{{ course.version }}</span>
        </div>
        <button
          class="group flex items-center gap-2 text-start"
          :title="$t('instructor.compose.editMeta')"
          @click="openMetaEditor"
        >
          <h1 class="text-2xl font-bold text-foreground truncate">{{ course.title }}</h1>
          <svg class="h-4 w-4 shrink-0 text-muted-foreground opacity-0 transition-opacity group-hover:opacity-100" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
            <path stroke-linecap="round" stroke-linejoin="round" d="M11 5H6a2 2 0 00-2 2v11a2 2 0 002 2h11a2 2 0 002-2v-5m-1.414-9.414a2 2 0 112.828 2.828L11.828 15H9v-2.828l8.586-8.586z" />
          </svg>
        </button>
        <p v-if="course.description" class="mt-1 text-sm text-muted-foreground line-clamp-2">
          {{ course.description }}
        </p>
      </div>
      <div class="flex flex-wrap gap-2">
        <AppButton type="button"
          v-if="course.status === 'draft'"
          :loading="publishing"
          :disabled="savingStudio"
          :title="publishBlockers.join(' ')"
          @click="switchStage('review')"
        >
          {{ t('instructor.studio.reviewPublish') }}
        </AppButton>
        <AppButton type="button" variant="secondary" size="sm" @click="router.push(`/instructor/courses/${course.id}/learners`)">{{ t('instructor.studio.insights') }}</AppButton>
        <AppButton type="button" variant="ghost" size="sm" @click="openMetaEditor">{{ t('instructor.studio.settings') }}</AppButton>
      </div>
    </div>

    <section class="overflow-hidden rounded-xl border border-border bg-card" :aria-label="t('instructor.studio.courseCreator')">
      <nav :aria-label="t('instructor.studio.creationStages')" class="border-b border-border">
        <ol class="grid grid-cols-4 divide-x divide-border">
          <li v-for="(item, index) in stages" :key="item.id" class="min-w-0">
            <button type="button" class="flex min-h-20 w-full flex-col items-center justify-center gap-2 px-1 py-3 text-center sm:flex-row sm:justify-start sm:px-4 sm:text-start" :class="stage === item.id ? 'bg-primary/10 text-primary shadow-[inset_0_-3px_0_var(--app-primary)]' : 'hover:bg-muted/40 text-muted-foreground'" :aria-current="stage === item.id ? 'step' : undefined" aria-controls="course-stage-content" :disabled="savingStudio" @click="switchStage(item.id)">
              <span class="flex h-7 w-7 shrink-0 items-center justify-center rounded-full text-xs font-semibold" :class="stage === item.id ? 'bg-primary text-primary-foreground' : item.done ? 'bg-success/10 text-success' : 'border border-border'">{{ item.done ? '✓' : index + 1 }}</span>
              <span class="min-w-0"><strong class="block text-[11px] sm:text-sm">{{ item.label }}</strong><small class="mt-1 hidden text-xs font-normal text-muted-foreground sm:block">{{ item.status }}</small></span>
            </button>
          </li>
        </ol>
      </nav>
      <div id="course-stage-content" role="region" :aria-label="stages[stageIndex]?.label" class="space-y-5 p-4 sm:p-6">
      <form v-if="stage === 'brief' && courseStudio" class="max-w-3xl space-y-5" @submit.prevent="saveStudio">
        <header><h2 class="text-xl font-semibold">{{ t('instructor.studio.briefHeading') }}</h2><p class="mt-2 text-sm text-muted-foreground">{{ t('instructor.studio.briefHelp') }}</p></header>
        <AppInput v-model="courseStudio.value.audience" :label="t('instructor.studio.audience')" :maxlength="4000" />
        <AppInput v-model="courseStudio.value.prerequisites" :label="t('instructor.studio.prerequisites')" :maxlength="4000" />
        <AppTextarea v-model="courseStudio.value.outcome" :label="t('instructor.studio.outcome')" :rows="3" :maxlength="8000" />
        <AppTextarea v-model="courseStudio.value.initial_prompt" :label="t('instructor.studio.coursePrompt')" :rows="5" :maxlength="16000" />
        <p class="text-xs text-muted-foreground">{{ t('instructor.studio.coursePromptHelp') }}</p>
        <AppButton type="submit" :loading="savingStudio" :disabled="!studioDirty">{{ t('common.actions.save') }}</AppButton>
      </form>
      <section v-if="stage === 'sources' && courseStudio" class="space-y-5">
        <header class="flex flex-wrap items-center justify-between gap-3"><div><h2 class="text-xl font-semibold">{{ t('instructor.studio.sources') }}</h2><p class="mt-2 text-sm text-muted-foreground">{{ t('instructor.studio.sourceHelp') }}</p></div><AppButton type="button" :disabled="courseStudio.value.sources.length >= 30" @click="addSource">{{ t('instructor.studio.addSource') }}</AppButton></header>
        <div v-for="(source, index) in courseStudio.value.sources" :key="source.id" class="rounded-lg border border-border p-4 space-y-4">
          <div class="flex items-center justify-between gap-3"><label class="flex items-center gap-2 text-sm"><input v-model="source.selected" type="checkbox">{{ t('instructor.studio.includeSource') }}</label><AppButton type="button" variant="ghost" size="sm" @click="courseStudio.value.sources.splice(index, 1)">{{ t('common.actions.delete') }}</AppButton></div>
          <AppInput v-model="source.title" :label="t('instructor.studio.sourceTitle')" :maxlength="200" />
          <AppTextarea v-model="source.text" :label="t('instructor.studio.sourceText')" :rows="5" :maxlength="32000" />
        </div>
        <AppButton type="button" :loading="savingStudio" :disabled="!studioDirty" @click="saveStudio">{{ t('common.actions.save') }}</AppButton>
      </section>
      <section v-if="stage === 'review'" class="space-y-5">
        <header><h2 class="text-xl font-semibold">{{ t('instructor.studio.reviewHeading') }}</h2><p class="mt-2 text-sm text-muted-foreground">{{ t('instructor.studio.reviewHelp') }}</p></header>
        <p class="text-sm">{{ courseStudio?.value.outcome }}</p>
        <div class="flex flex-wrap gap-3"><AppButton type="button" variant="secondary" @click="switchStage('curriculum')">{{ t('instructor.studio.reviewLessons') }}</AppButton><AppButton type="button" variant="secondary" @click="router.push(`/courses/${course.id}`)">{{ t('instructor.studio.preview') }}</AppButton></div>
        <div class="rounded-xl border border-border bg-card">
          <div class="border-b border-border p-4"><h3 class="font-semibold">{{ t('instructor.studio.learnerFeedback') }}</h3><p class="mt-1 text-xs text-muted-foreground">{{ t('instructor.studio.learnerFeedbackHelp') }}</p></div>
          <div v-if="lessonFeedback.length" class="divide-y divide-border">
            <div v-for="item in lessonFeedback" :key="item.id" class="flex flex-wrap items-start gap-4 p-4">
              <div class="min-w-0 flex-1"><div class="flex flex-wrap items-center gap-2"><span class="text-sm font-medium">{{ item.element_title }}</span><span class="rounded-full bg-muted px-2 py-0.5 text-xs">{{ t('instructor.studio.ratingOutOfFive', { rating: item.rating }) }}</span></div><p v-if="item.comment" class="mt-2 text-sm text-muted-foreground">{{ item.comment }}</p></div>
              <AppButton type="button" variant="secondary" size="sm" @click="improveFromFeedback(item)">{{ t('instructor.studio.improveWithAi') }}</AppButton>
            </div>
          </div>
          <p v-else class="p-4 text-sm text-muted-foreground">{{ t('instructor.studio.noLearnerFeedback') }}</p>
        </div>
        <label class="flex items-start gap-3 text-sm"><input v-model="releaseReviewed" type="checkbox" class="mt-1">{{ t('instructor.studio.releaseReviewed') }}</label>
        <AppButton type="button" :disabled="!releaseReviewed || publishBlockers.length > 0" :loading="publishing" @click="showPublishConfirm = true">{{ t('instructor.studio.reviewPublish') }}</AppButton>
      </section>
    <!-- Publish blockers / result -->
    <div v-if="stage === 'review' && publishBlockers.length" class="rounded-lg border border-warning/30 bg-warning/5 px-4 py-3">
      <p class="text-xs font-semibold uppercase tracking-wide text-warning mb-1">{{ $t('instructor.compose.beforePublish') }}</p>
      <ul class="text-sm text-warning list-disc ps-4">
        <li v-for="b in publishBlockers" :key="b">{{ b }}</li>
      </ul>
    </div>
    <div v-if="publishResult" class="rounded-lg border border-success/20 bg-success/10 px-4 py-3">
      <p class="text-sm font-medium text-success">{{ $t('instructor.compose.publishedTitle') }}</p>
      <p class="text-xs text-muted-foreground">{{ $t('instructor.compose.publishedNote') }}</p>
      <details class="mt-1">
        <summary class="cursor-pointer text-xs text-muted-foreground">{{ $t('common.advanced.toggle') }}</summary>
        <p class="mt-1 text-xs text-muted-foreground">
          {{ $t('instructor.compose.fingerprintLabel') }}:
          <code class="font-mono">{{ publishResult.content_hash }}</code>
          ({{ $t('instructor.compose.sizeBytes', { count: publishResult.size }, publishResult.size) }})
        </p>
      </details>
    </div>
    <p v-if="error" class="text-sm text-error">{{ error }}</p>

    <!-- Two-pane: outline | editor -->
    <div v-if="stage === 'curriculum'" class="space-y-5">
      <div class="flex flex-wrap items-center justify-between gap-3"><h2 class="font-semibold">{{ t('instructor.studio.curriculum') }}</h2><AppButton type="button" variant="secondary" :disabled="!workflowSupported" @click="openRunSetup">{{ t('instructor.studio.runWorkflow') }}</AppButton></div>
      <form v-if="showRunSetup" class="rounded-xl border border-border p-5 space-y-4" @submit.prevent="prepareRun">
        <label class="block text-sm"><span class="mb-2 block">{{ t('instructor.studio.workflowName') }}</span><select v-model="selectedWorkflow" required class="w-full rounded-lg border border-border bg-background p-2"><option v-for="item in studio.workflows.value" :key="item.id" :value="item.id">{{ item.value.name }}</option></select></label>
        <AppTextarea v-model="initialPrompt" :label="t('instructor.studio.runPrompt')" :rows="3" :maxlength="16000" />
        <p class="text-xs text-muted-foreground">{{ t('instructor.studio.prepareHelp') }}</p>
        <div class="flex flex-wrap gap-2"><AppButton type="submit" :disabled="!selectedWorkflow" :loading="preparingRun">{{ t('instructor.studio.prepareRun') }}</AppButton><AppButton type="button" variant="ghost" @click="router.push('/instructor/workflows')">{{ t('instructor.studio.manageWorkflows') }}</AppButton></div>
      </form>
      <StudioRunPanel v-if="activeRun" :run-id="activeRun" :draft-dirty="editorDirty" @applied="reloadAfterRun" @close="activeRun = null" />
      <div class="grid gap-5 lg:grid-cols-[240px_minmax(0,1fr)]">
      <OutlinePanel
        :course-id="course.id"
        :chapters="chapters"
        :elements="elements"
        :selected-element-id="selectedElement?.id ?? null"
        :flat="kind === 'tutorial'"
        @select="selectElement"
        @changed="reloadOutline"
      />

      <div class="rounded-xl border border-border bg-card p-5">
        <ElementEditorHost
          v-if="selectedElement"
          :key="`${selectedElement.id}:${editorRevision}`"
          :element="selectedElement"
          @dirty="editorDirty = $event"
          @updated="onElementUpdated"
          @deleted="onElementDeleted"
        />
        <EmptyState
          v-else
          :title="$t('instructor.compose.nothingSelectedTitle')"
          :description="$t('instructor.compose.nothingSelectedDesc')"
        />
      </div>
    </div>

      </div>
      </div>
      <footer class="flex flex-wrap items-center justify-between gap-3 border-t border-border bg-card px-4 py-4 sm:px-6">
        <span class="text-xs text-muted-foreground">{{ t('instructor.studio.stageCount', { current: stageIndex + 1, total: 4 }) }} · {{ t(studioDirty || editorDirty ? 'instructor.studio.unsaved' : 'instructor.studio.saved') }}</span>
        <div class="flex gap-2"><AppButton type="button" v-if="stageIndex > 0" variant="secondary" :disabled="savingStudio" @click="switchStage(stages[stageIndex - 1]!.id)">{{ t('common.actions.back') }}</AppButton><AppButton type="button" v-if="stageIndex < 3" :loading="savingStudio" @click="switchStage(stages[stageIndex + 1]!.id)">{{ t('instructor.studio.continueTo', { stage: stages[stageIndex + 1]?.label }) }}</AppButton></div>
      </footer>
    </section>
    <!-- Dialogs -->
    <ConfirmDialog :open="discardEditor" :title="t('instructor.studio.unsaved')" :message="t('instructor.studio.discardMessage')" :confirm-label="t('instructor.studio.discard')" @confirm="resolveEditorLeave(true)" @cancel="resolveEditorLeave(false)" />
    <ConfirmDialog
      :open="showPublishConfirm"
      :title="$t('instructor.compose.confirmPublishTitle')"
      :message="$t('instructor.compose.confirmPublishMessage', { title: course.title })"
      :confirm-label="$t('instructor.compose.publish')"
      :loading="publishing"
      @confirm="publish"
      @cancel="showPublishConfirm = false"
    />
    <ConfirmDialog
      :open="showDeleteConfirm"
      :title="$t('instructor.compose.confirmDeleteTitle')"
      :message="$t('instructor.compose.confirmDeleteMessage')"
      :confirm-label="$t('common.actions.delete')"
      confirm-variant="danger"
      :loading="deleting"
      @confirm="deleteCourse"
      @cancel="showDeleteConfirm = false"
    />

    <!-- Metadata editor -->
    <Teleport to="body">
      <div v-if="editingMeta" class="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4" @click.self="closeMetaEditor">
        <div class="w-full max-w-2xl rounded-xl border border-border bg-card p-6 space-y-5">
          <div>
            <h2 class="text-lg font-semibold text-foreground">{{ t('instructor.studio.courseSettings') }}</h2>
            <p class="mt-1 text-sm text-muted-foreground">{{ t('instructor.studio.courseSettingsHelp') }}</p>
          </div>
          <AppTabs v-model="metaTab" :tabs="[
            { key: 'details', label: t('instructor.studio.courseDetails') },
            { key: 'tutor', label: t('instructor.studio.learnerAssistance') },
          ]" />
          <div v-if="metaTab === 'details'" class="space-y-4">
            <AppInput v-model="metaTitle" :label="$t('instructor.compose.titleLabel')" />
            <AppTextarea v-model="metaDescription" :label="$t('instructor.compose.descriptionLabel')" :rows="4" />
          </div>
          <div v-else-if="courseStudio" class="space-y-5">
            <label class="flex items-start gap-3 rounded-lg border border-border bg-muted/20 p-4">
              <input v-model="metaTutor.enabled" type="checkbox" class="mt-1">
              <span><span class="block text-sm font-medium">{{ t('instructor.studio.enableTutor') }}</span><span class="mt-1 block text-xs text-muted-foreground">{{ t('instructor.studio.enableTutorHelp') }}</span></span>
            </label>
            <label class="block text-sm">
              <span class="mb-2 block">{{ t('instructor.studio.tutorGuidance') }}</span>
              <select v-model="metaTutor.guidance" :disabled="!metaTutor.enabled" class="w-full rounded-lg border border-border bg-background p-2.5 disabled:opacity-50">
                <option value="socratic">{{ t('instructor.studio.tutorSocratic') }}</option>
                <option value="balanced">{{ t('instructor.studio.tutorBalanced') }}</option>
                <option value="direct">{{ t('instructor.studio.tutorDirect') }}</option>
              </select>
            </label>
            <AppTextarea v-model="metaTutor.initial_prompt" :disabled="!metaTutor.enabled" :label="t('instructor.studio.tutorPrompt')" :rows="6" :maxlength="16000" />
            <p class="text-xs text-muted-foreground">{{ t('instructor.studio.tutorPromptHelp') }}</p>
          </div>
          <div class="flex justify-end gap-2">
            <AppButton v-if="metaTab === 'details'" type="button" variant="danger" @click="editingMeta = false; showDeleteConfirm = true">{{ t('common.actions.delete') }}</AppButton>
            <AppButton type="button" variant="ghost" @click="closeMetaEditor">{{ $t('common.actions.cancel') }}</AppButton>
            <AppButton type="button" :loading="savingMeta || savingStudio" @click="saveMeta">{{ $t('common.actions.save') }}</AppButton>
          </div>
        </div>
      </div>
    </Teleport>
  </div>

  <!-- ── Not found ─────────────────────────────────────────────── -->
  <EmptyState
    v-else
    :title="$t('instructor.compose.notFoundTitle')"
    :description="$t('instructor.compose.notFoundDesc')"
  />
</template>
