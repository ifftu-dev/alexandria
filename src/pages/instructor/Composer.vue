<script setup lang="ts">
// Unified course & tutorial composer.
//
// One surface for both kinds: outline on the left (chapters hidden for
// tutorials — one implicit chapter), the selected element's editor on
// the right. Draft ↔ publish lifecycle wraps the existing
// `publish_course` pipeline.
import { computed, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'
import { useLocalApi } from '@/composables/useLocalApi'
import { AppButton, AppInput, AppTextarea, ConfirmDialog, EmptyState, StatusBadge } from '@/components/ui'
import OutlinePanel from '@/components/composer/OutlinePanel.vue'
import ElementEditorHost from '@/components/composer/ElementEditorHost.vue'
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

const isNew = computed(() => route.params.id === undefined)
const kind = computed<'course' | 'tutorial'>(() => {
  if (course.value) return course.value.kind === 'tutorial' ? 'tutorial' : 'course'
  return route.query.kind === 'tutorial' ? 'tutorial' : 'course'
})

const course = ref<Course | null>(null)
const chapters = ref<Chapter[]>([])
const elements = ref<Record<string, Element[]>>({})
const selectedElement = ref<Element | null>(null)
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
    router.replace(`/instructor/composer/${created.id}`)
    await load(created.id)
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
    await reloadOutline()
  } catch (e) {
    error.value = String(e)
    course.value = null
  } finally {
    loading.value = false
  }
}

async function reloadOutline() {
  if (!course.value) return
  chapters.value = await invoke<Chapter[]>('list_chapters', { courseId: course.value.id })
  const map: Record<string, Element[]> = {}
  for (const ch of chapters.value) {
    map[ch.id] = await invoke<Element[]>('list_elements', { chapterId: ch.id }).catch(() => [])
  }
  elements.value = map
  // Keep the selection fresh (it may have moved or been deleted).
  if (selectedElement.value) {
    const found = Object.values(map).flat().find(e => e.id === selectedElement.value?.id)
    selectedElement.value = found ?? null
  }
}

onMounted(() => {
  const id = route.params.id
  if (typeof id === 'string') void load(id)
})

watch(() => route.params.id, (id) => {
  if (typeof id === 'string') void load(id)
})

// ── Course metadata editing ─────────────────────────────────────
const editingMeta = ref(false)
const metaTitle = ref('')
const metaDescription = ref('')
const savingMeta = ref(false)

function openMetaEditor() {
  if (!course.value) return
  metaTitle.value = course.value.title
  metaDescription.value = course.value.description ?? ''
  editingMeta.value = true
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
    editingMeta.value = false
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
        <AppButton :loading="creating" @click="createDraft">{{ $t('instructor.compose.createDraft') }}</AppButton>
        <AppButton variant="ghost" @click="router.back()">{{ $t('common.actions.cancel') }}</AppButton>
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
  <div v-else-if="course" class="space-y-5">
    <!-- Header -->
    <div class="flex items-start justify-between gap-4">
      <div class="min-w-0">
        <div class="flex items-center gap-2 mb-1.5">
          <StatusBadge :status="course.status" />
          <span class="rounded-full bg-muted px-2 py-0.5 text-[10px] font-medium uppercase tracking-wide text-muted-foreground">
            {{ kind }}
          </span>
          <span class="text-xs text-muted-foreground">v{{ course.version }}</span>
        </div>
        <button
          class="group flex items-center gap-2 text-left"
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
      <div class="flex shrink-0 gap-2">
        <AppButton
          v-if="course.status === 'draft'"
          :loading="publishing"
          :disabled="publishBlockers.length > 0"
          :title="publishBlockers.join(' ')"
          @click="showPublishConfirm = true"
        >
          {{ $t('instructor.compose.publish') }}
        </AppButton>
        <AppButton variant="danger" size="sm" @click="showDeleteConfirm = true">{{ $t('common.actions.delete') }}</AppButton>
      </div>
    </div>

    <!-- Publish blockers / result -->
    <div v-if="course.status === 'draft' && publishBlockers.length" class="rounded-lg border border-warning/30 bg-warning/5 px-4 py-3">
      <p class="text-xs font-semibold uppercase tracking-wide text-warning mb-1">{{ $t('instructor.compose.beforePublish') }}</p>
      <ul class="text-sm text-warning list-disc pl-4">
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
    <div class="grid gap-5 lg:grid-cols-[300px_minmax(0,1fr)]">
      <OutlinePanel
        :course-id="course.id"
        :chapters="chapters"
        :elements="elements"
        :selected-element-id="selectedElement?.id ?? null"
        :flat="kind === 'tutorial'"
        @select="selectedElement = $event"
        @changed="reloadOutline"
      />

      <div class="rounded-xl border border-border bg-card p-5">
        <ElementEditorHost
          v-if="selectedElement"
          :element="selectedElement"
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

    <!-- Dialogs -->
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
      <div v-if="editingMeta" class="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4" @click.self="editingMeta = false">
        <div class="w-full max-w-lg rounded-xl border border-border bg-card p-6 space-y-4">
          <h2 class="text-lg font-semibold text-foreground">{{ $t('instructor.compose.editDetails') }}</h2>
          <AppInput v-model="metaTitle" :label="$t('instructor.compose.titleLabel')" />
          <AppTextarea v-model="metaDescription" :label="$t('instructor.compose.descriptionLabel')" :rows="4" />
          <div class="flex justify-end gap-2">
            <AppButton variant="ghost" @click="editingMeta = false">{{ $t('common.actions.cancel') }}</AppButton>
            <AppButton :loading="savingMeta" @click="saveMeta">{{ $t('common.actions.save') }}</AppButton>
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
