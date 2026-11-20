<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { Channel } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { useLocalApi } from '@/composables/useLocalApi'
import { AppButton, AppSpinner } from '@/components/ui'
import type {
  Cmi5Launch,
  Element,
  ElementProgress,
  OpenedPackageLaunch,
  Package,
  PackageActivity,
  PackageActivityResult,
  PackageAttempt,
  PackageAttemptProgress,
  PackageExtractProgress,
} from '@/types'
import {
  createPackageSession,
  PACKAGE_FRAME_SANDBOX,
  type BlockedKind,
  type NavigationRequest,
  type PackageInvoke,
  type PackageSession,
  type SequencingNavigation,
  type SequencingNode,
} from './packageSession'
import { previewInvoke } from './packagePreview'
import { launchables, outline } from './packageTree'

// A package element: a SCORM package running in the wrapper at the package
// origin, or cmi5 units framed by it, under a label the app draws. Results come from the package's own
// script and are shown as self-reported; the host writes element progress
// and this component passes it up. See `packageSession.ts` for the protocol.

const props = defineProps<{
  element: Element
  enrollmentId: string | null
  /** Who published the course, or `null` for a package file opened on this device. */
  publisher: string | null
  progress: ElementProgress | null
  /** An author's preview: runs without an enrolment and stores nothing. */
  preview?: boolean
}>()

const emit = defineEmits<{ progress: [progress: ElementProgress] }>()

const { t } = useI18n()
const { invoke: localInvoke } = useLocalApi()
// A preview runs the same lifecycle against the host's preview commands:
// no enrolment, nothing stored.
const invoke: PackageInvoke = props.preview ? previewInvoke(localInvoke, props.element.id) : localInvoke

type Phase = 'loading' | 'unenrolled' | 'missing' | 'preparing' | 'ready' | 'error'
interface Row { activity: PackageActivity, depth: number }
interface Blocked { kind: BlockedKind, detail: string | null }

const frame = ref<HTMLIFrameElement | null>(null)
const phase = ref<Phase>('loading')
const loadError = ref<string | null>(null)
const extracting = ref<PackageExtractProgress | null>(null)
const pkg = ref<Package | null>(null)
const attempt = ref<PackageAttempt | null>(null)
const results = ref<PackageActivityResult[]>([])
const currentId = ref<string | null>(null)
const elementProgress = ref<ElementProgress | null>(props.progress)
const saveError = ref<string | null>(null)
const blocked = ref<Blocked[]>([])
const focus = ref(false)
const confirmingFresh = ref(false)
const busy = ref(false)
/** A lesson is opening: the lesson controls wait for it. */
const opening = ref(false)
let openings = 0
/** cmi5: the running unit's own window, while it is open (desktop). */
const unitWindow = ref<Cmi5Launch | null>(null)
/** cmi5: the running unit returned to its return page. */
const unitExited = ref(false)
/** cmi5: the running unit asked for its own window on mobile; focus mode stays on. */
const focusForced = ref(false)
/**
 * A new frame element for every session: a frame keeps its name and its
 * history (which holds the earlier launch's addresses) across navigations.
 */
const frameKey = ref(0)
let session: PackageSession | null = null
let disposed = false
let stopListening: UnlistenFn | null = null

// SCORM 2004: the sequencing engine in the wrapper decides what is delivered
// and which controls apply; the player draws what it reports.
type Sequenced = 'running' | 'choose' | 'ended'
interface SequencedRow { node: SequencingNode, depth: number, lesson: boolean }
const sequenced = computed(() => pkg.value?.standard === 'scorm2004')
const navigation = ref<SequencingNavigation | null>(null)
const sequencedPhase = ref<Sequenced>('running')
const endedReason = ref<string | null>(null)
const confirmingExit = ref(false)

const sequencedRows = computed<SequencedRow[]>(() =>
  outline(navigation.value?.tree?.children).map(({ item, depth }) => ({ node: item, depth, lesson: item.children.length === 0 })))
const sequencedLessonCount = computed(() => sequencedRows.value.filter(row => row.lesson).length)
const hidden = (control: string) => navigation.value?.hidden.includes(control) ?? false

function request(kind: NavigationRequest, target?: string) {
  confirmingExit.value = false
  session?.navigate(kind, target)
}

/** The table of contents: visible activities in order, headings included. */
const rows = computed<Row[]>(() =>
  outline(pkg.value?.description.tree.children).map(({ item, depth }) => ({ activity: item, depth })))

const lessons = computed<PackageActivity[]>(() => launchables(pkg.value?.description.tree))

const currentIndex = computed(() => lessons.value.findIndex(lesson => lesson.id === currentId.value))
const resultFor = (id: string) => results.value.find(result => result.activity_id === id)

const status = computed(() => {
  const value = elementProgress.value?.status
  if (value === 'completed') return t('learn.package.status.completed')
  if (value === 'in_progress') return t('learn.package.status.inProgress')
  return t('learn.package.status.notStarted')
})
const scorePercent = computed(() => {
  const score = elementProgress.value?.score
  return score == null ? null : Math.round(score * 100)
})

/** Where a launch picks up: a lesson left part-way, else the first not yet started. */
function resumeLesson(): PackageActivity | undefined {
  const all = lessons.value
  return all.find(lesson => resultFor(lesson.id)?.started && !resultFor(lesson.id)?.complete)
    ?? all.find(lesson => !resultFor(lesson.id)?.started)
    ?? all[0]
}

function onProgress(progress: PackageAttemptProgress) {
  if (attempt.value && progress.attempt.id === attempt.value.id) attempt.value = progress.attempt
  elementProgress.value = progress.element_progress
  emit('progress', progress.element_progress)
  if (!disposed && attempt.value?.id === progress.attempt.id) void refreshResults()
}

async function refreshResults() {
  const id = attempt.value?.id
  if (!id) return
  try {
    const next = await invoke<PackageActivityResult[]>('package_attempt_activities', { attemptId: id })
    if (attempt.value?.id === id) results.value = next
  } catch (error) {
    console.warn('package lesson results failed:', error)
  }
}

function addBlocked(kind: BlockedKind, detail: string | null) {
  if (blocked.value.some(item => item.kind === kind && item.detail === detail)) return
  blocked.value = [...blocked.value, { kind, detail }]
}

function blockedMessage(item: Blocked): string {
  switch (item.kind) {
    case 'popup': return t('learn.package.blocked.popup')
    case 'dialog': return t('learn.package.blocked.dialog')
    case 'plugin': return t('learn.package.blocked.plugin')
    case 'remote-resource':
      return item.detail ? t('learn.package.blocked.remote', { host: item.detail }) : t('learn.package.blocked.remoteUnknown')
  }
  return ''
}

async function startSession() {
  const hash = props.element.content_cid
  const current = attempt.value
  if (!hash || !current || !pkg.value) return
  phase.value = 'preparing'
  frameKey.value += 1
  extracting.value = null
  const channel = new Channel<PackageExtractProgress>()
  channel.onmessage = (progress) => { extracting.value = progress }
  const launch = await invoke<OpenedPackageLaunch>('package_launch_open', { hash, onProgress: channel })
  await nextTick()
  if (disposed || !frame.value) {
    await invoke('package_launch_close', { prefix: launch.prefix })
    return
  }
  const opened = createPackageSession({
    invoke,
    frame: frame.value,
    launch,
    attemptId: current.id,
    standard: pkg.value.standard,
    events: {
      progress: onProgress,
      saveError: (message) => { saveError.value = message },
      blocked: addBlocked,
      focusRequest: () => { focus.value = true },
      refused: (reason) => {
        phase.value = 'error'
        loadError.value = reason === 'unsupported-standard' ? t('learn.package.unsupported') : reason
      },
      unitExited: () => {
        unitExited.value = true
        void endUnit()
      },
      unitWindow: (launch) => { unitWindow.value = launch },
      navigation: (state) => { navigation.value = state },
      delivered: (activityId) => {
        currentId.value = activityId
        sequencedPhase.value = 'running'
        endedReason.value = null
      },
      undelivered: () => {
        currentId.value = null
        sequencedPhase.value = 'choose'
      },
      sessionEnded: (reason) => {
        currentId.value = null
        sequencedPhase.value = 'ended'
        endedReason.value = reason
      },
    },
  })
  session = opened
  await opened.start()
  await refreshResults()
  if (disposed) return
  phase.value = 'ready'
  const tree = pkg.value?.description.tree
  if (sequenced.value && tree) {
    await opened.startSequencing(tree)
    return
  }
  const first = resumeLesson()
  if (first) await openLesson(first)
}

async function openLesson(activity: PackageActivity) {
  if (!session || !activity.launch) return
  currentId.value = activity.id
  unitExited.value = false
  const request = ++openings
  opening.value = true
  try {
    const opened = await session.openLesson(activity)
    // A later pick overtook this one; it sets up the player itself.
    if (!opened) return
    focusForced.value = 'focus_forced' in opened && opened.focus_forced
    if (focusForced.value) focus.value = true
    void refreshResults()
  } catch (error) {
    loadError.value = String(error)
    phase.value = 'error'
  } finally {
    if (request === openings) opening.value = false
  }
}

/** cmi5: ends the running unit's session (its own window closes with it). */
async function endUnit() {
  await session?.endLesson()
  focusForced.value = false
  void refreshResults()
}

/** cmi5: brings the unit's own window forward; a window already gone ends the unit. */
async function focusUnitWindow() {
  const open = await session?.focusUnitWindow()
  if (open === false) await endUnit()
}

function step(offset: number) {
  const next = lessons.value[currentIndex.value + offset]
  if (next) void openLesson(next)
}

async function startFresh() {
  if ((!props.enrollmentId && !props.preview) || busy.value) return
  busy.value = true
  confirmingFresh.value = false
  try {
    const previous = session
    session = null
    await previous?.close()
    attempt.value = await invoke<PackageAttempt>('package_attempt_fresh', {
      enrollmentId: props.enrollmentId,
      elementId: props.element.id,
    })
    results.value = []
    blocked.value = []
    saveError.value = null
    navigation.value = null
    sequencedPhase.value = 'running'
    endedReason.value = null
    await startSession()
  } catch (error) {
    loadError.value = String(error)
    phase.value = 'error'
  } finally {
    busy.value = false
  }
}

function onKeydown(event: KeyboardEvent) {
  if (event.key === 'Escape' && focus.value && !focusForced.value) focus.value = false
}

onMounted(async () => {
  window.addEventListener('keydown', onKeydown)
  // cmi5: the host ends a session itself when the unit's own window closes.
  void listen<{ session_id: string, progress?: PackageAttemptProgress | null }>('package-cmi5-ended', (event) => {
    session?.unitEnded(event.payload.session_id, event.payload.progress ?? null)
    focusForced.value = false
  }).then((unlisten) => {
    if (disposed) unlisten()
    else stopListening = unlisten
  }).catch(() => {})
  const hash = props.element.content_cid
  if (!props.enrollmentId && !props.preview) {
    phase.value = 'unenrolled'
    return
  }
  if (!hash) {
    phase.value = 'missing'
    return
  }
  try {
    const stored = await invoke<Package | null>('package_get', { hash })
    if (disposed) return
    if (!stored) {
      phase.value = 'missing'
      return
    }
    pkg.value = stored
    attempt.value = await invoke<PackageAttempt>('package_attempt_open', {
      enrollmentId: props.enrollmentId,
      elementId: props.element.id,
    })
    if (disposed) return
    await startSession()
  } catch (error) {
    if (disposed) return
    loadError.value = String(error)
    phase.value = 'error'
  }
})

onBeforeUnmount(() => {
  disposed = true
  window.removeEventListener('keydown', onKeydown)
  stopListening?.()
  const closing = session
  session = null
  // The frame goes with this component; the wrapper finishes the lesson as
  // its page unloads and the session still receives its last commit.
  void closing?.close().catch(error => console.warn('package close failed:', error))
})
</script>

<template>
  <div
    data-package-player
    :data-focus="focus ? 'true' : 'false'"
    class="flex min-h-0 flex-col bg-background"
    :class="focus ? 'fixed inset-0 z-[60]' : 'flex-1'"
  >
    <!-- Drawn by the app, outside the frame, in and out of focus mode. -->
    <div
      data-package-label
      role="note"
      :aria-label="t('learn.package.label.region')"
      class="flex shrink-0 items-center gap-3 border-b border-amber-500/40 bg-amber-50 px-3 py-1.5 text-xs text-amber-900 dark:bg-amber-950/60 dark:text-amber-100"
    >
      <svg class="h-4 w-4 shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2" aria-hidden="true">
        <path stroke-linecap="round" stroke-linejoin="round" d="M12 9v3.75m0 3.75h.008M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
      </svg>
      <p class="min-w-0 flex-1">
        <span class="font-medium">{{ publisher ? t('learn.package.label.publisher', { publisher }) : t('learn.package.label.file') }}</span>
        {{ ' ' }}{{ t('learn.package.label.phrase') }}
      </p>
      <button
        type="button"
        class="pressable shrink-0 rounded-md border border-amber-600/40 px-2 py-0.5 font-medium hover:bg-amber-100 dark:hover:bg-amber-900/50"
        :aria-pressed="focus"
        :disabled="focusForced"
        :title="focusForced ? t('learn.package.focusForced') : undefined"
        @click="focus = !focus"
      >
        {{ focus ? t('learn.package.label.focusOff') : t('learn.package.label.focusOn') }}
      </button>
    </div>

    <p v-if="preview" data-package-preview-note class="shrink-0 border-b border-border bg-muted/40 px-3 py-1 text-xs text-muted-foreground">
      {{ t('instructor.package.previewNote') }}
    </p>

    <div v-if="phase === 'unenrolled' || phase === 'missing' || phase === 'error'" class="m-4 rounded-lg border border-border bg-card p-6 text-center text-sm text-muted-foreground">
      <p v-if="phase === 'unenrolled'">{{ t('learn.package.unenrolled') }}</p>
      <p v-else-if="phase === 'missing'">{{ t('learn.package.missing') }}</p>
      <p v-else role="alert">{{ t('learn.package.loadFailed', { error: loadError ?? '' }) }}</p>
    </div>

    <div v-else class="flex min-h-0 flex-1 flex-col md:flex-row">
      <!-- Table of contents -->
      <!-- SCORM 2004: the engine's tree; a lesson is offered only when a choice of it is valid,
           and a cluster is offered when a choice of it is (the engine then delivers a lesson in it). -->
      <nav
        v-if="!focus && sequenced && sequencedLessonCount > 1"
        data-package-toc
        :aria-label="t('learn.package.toc')"
        class="max-h-40 shrink-0 overflow-y-auto border-b border-border p-2 md:max-h-none md:w-60 md:border-b-0 md:border-e"
      >
        <p class="px-2 pb-1 text-[11px] font-semibold uppercase tracking-wide text-muted-foreground">{{ t('learn.package.toc') }}</p>
        <template v-for="row in sequencedRows" :key="row.node.id">
          <button
            v-if="row.lesson || row.node.choosable"
            type="button"
            :data-activity="row.node.id"
            :data-complete="row.node.completion === 'completed' ? 'true' : 'false'"
            :aria-current="row.node.id === currentId ? 'true' : undefined"
            :disabled="phase !== 'ready' || busy || !row.node.choosable"
            class="pressable selection-indicator flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-start text-sm hover:bg-muted/50 disabled:cursor-not-allowed disabled:opacity-50 aria-[current=true]:bg-primary/10 aria-[current=true]:font-medium"
            :style="{ paddingInlineStart: `${0.5 + row.depth * 0.75}rem` }"
            @click="request('choice', row.node.id)"
          >
            <span
              class="flex h-4 w-4 shrink-0 items-center justify-center rounded-full border"
              :class="row.node.completion === 'completed' ? 'border-emerald-500 bg-emerald-500 text-white' : row.node.completion === 'incomplete' ? 'border-amber-500' : 'border-border'"
              aria-hidden="true"
            >
              <svg v-if="row.node.completion === 'completed'" class="h-3 w-3" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="3">
                <path stroke-linecap="round" stroke-linejoin="round" d="M5 13l4 4L19 7" />
              </svg>
            </span>
            <span class="min-w-0 flex-1 truncate">{{ row.node.title }}</span>
          </button>
          <p
            v-else
            class="truncate px-2 pt-2 text-xs font-medium text-muted-foreground"
            :style="{ paddingInlineStart: `${0.5 + row.depth * 0.75}rem` }"
          >
            {{ row.node.title }}
          </p>
        </template>
      </nav>

      <nav
        v-if="!focus && !sequenced && lessons.length > 1"
        data-package-toc
        :aria-label="t('learn.package.toc')"
        class="max-h-40 shrink-0 overflow-y-auto border-b border-border p-2 md:max-h-none md:w-60 md:border-b-0 md:border-e"
      >
        <p class="px-2 pb-1 text-[11px] font-semibold uppercase tracking-wide text-muted-foreground">{{ t('learn.package.toc') }}</p>
        <template v-for="row in rows" :key="row.activity.id">
          <button
            v-if="row.activity.launch"
            type="button"
            :data-activity="row.activity.id"
            :data-complete="resultFor(row.activity.id)?.complete ? 'true' : 'false'"
            :aria-current="row.activity.id === currentId ? 'true' : undefined"
            :disabled="phase !== 'ready' || busy || opening"
            class="pressable selection-indicator flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-start text-sm hover:bg-muted/50 aria-[current=true]:bg-primary/10 aria-[current=true]:font-medium"
            :style="{ paddingInlineStart: `${0.5 + row.depth * 0.75}rem` }"
            @click="openLesson(row.activity)"
          >
            <span
              class="flex h-4 w-4 shrink-0 items-center justify-center rounded-full border"
              :class="resultFor(row.activity.id)?.complete ? 'border-emerald-500 bg-emerald-500 text-white' : resultFor(row.activity.id)?.started ? 'border-amber-500' : 'border-border'"
              aria-hidden="true"
            >
              <svg v-if="resultFor(row.activity.id)?.complete" class="h-3 w-3" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="3">
                <path stroke-linecap="round" stroke-linejoin="round" d="M5 13l4 4L19 7" />
              </svg>
            </span>
            <span class="min-w-0 flex-1 truncate">{{ row.activity.title }}</span>
          </button>
          <p
            v-else
            class="truncate px-2 pt-2 text-xs font-medium text-muted-foreground"
            :style="{ paddingInlineStart: `${0.5 + row.depth * 0.75}rem` }"
          >
            {{ row.activity.title }}
          </p>
        </template>
      </nav>

      <div class="flex min-h-0 min-w-0 flex-1 flex-col">
        <!-- Result, attempt and controls. Focus mode hides the course's own chrome but keeps
             a SCORM 2004 package's navigation, which the package's rules may need to leave a lesson. -->
        <div
          v-if="!focus || sequenced"
          data-package-controls
          class="flex shrink-0 flex-wrap items-center gap-x-3 gap-y-1 border-b border-border px-3 text-xs"
          :class="focus ? 'justify-end py-1' : 'py-2'"
        >
          <p v-if="!focus" data-package-result class="min-w-0 flex-1 text-muted-foreground">
            <span class="font-medium text-foreground">{{ status }}</span>
            <template v-if="scorePercent !== null"> · {{ t('learn.package.score', { score: scorePercent }) }}</template>
            · {{ t('learn.package.selfReported') }}
          </p>
          <span v-if="!focus" data-package-attempt class="text-muted-foreground">{{ t('learn.package.attempt', { number: attempt?.attempt_number ?? 1 }) }}</span>
          <template v-if="sequenced">
            <AppButton
              v-if="!hidden('previous')"
              data-package-nav="previous"
              size="xs"
              variant="ghost"
              :disabled="phase !== 'ready' || !navigation?.previous"
              @click="request('previous')"
            >
              {{ t('learn.package.previous') }}
            </AppButton>
            <AppButton
              v-if="!hidden('continue')"
              data-package-nav="continue"
              size="xs"
              variant="ghost"
              :disabled="phase !== 'ready' || !navigation?.continue"
              @click="request('continue')"
            >
              {{ t('learn.package.continue') }}
            </AppButton>
            <AppButton
              v-if="!hidden('suspendAll')"
              data-package-nav="suspend"
              size="xs"
              variant="ghost"
              :disabled="phase !== 'ready' || !navigation?.active"
              @click="request('suspendAll')"
            >
              {{ t('learn.package.suspend') }}
            </AppButton>
            <AppButton
              v-if="!hidden('exitAll')"
              data-package-nav="exit"
              size="xs"
              variant="ghost"
              :disabled="phase !== 'ready' || !navigation?.active"
              @click="confirmingExit = true"
            >
              {{ t('learn.package.exit') }}
            </AppButton>
          </template>
          <template v-else-if="lessons.length > 1">
            <AppButton size="xs" variant="ghost" :disabled="phase !== 'ready' || opening || currentIndex <= 0" @click="step(-1)">
              {{ t('learn.package.previousLesson') }}
            </AppButton>
            <AppButton size="xs" variant="ghost" :disabled="phase !== 'ready' || opening || currentIndex < 0 || currentIndex >= lessons.length - 1" @click="step(1)">
              {{ t('learn.package.nextLesson') }}
            </AppButton>
          </template>
          <AppButton v-if="!focus" data-package-fresh size="xs" variant="secondary" :disabled="phase !== 'ready' || busy" @click="confirmingFresh = true">
            {{ t('learn.package.fresh') }}
          </AppButton>
        </div>

        <div v-if="confirmingFresh && !focus" class="flex shrink-0 flex-wrap items-center gap-2 border-b border-border bg-muted/30 px-3 py-2 text-xs">
          <p class="min-w-0 flex-1">{{ t('learn.package.freshBody') }}</p>
          <AppButton data-package-fresh-confirm size="xs" @click="startFresh">{{ t('learn.package.freshConfirm') }}</AppButton>
          <AppButton size="xs" variant="ghost" @click="confirmingFresh = false">{{ t('learn.package.freshCancel') }}</AppButton>
        </div>

        <div v-if="confirmingExit" class="flex shrink-0 flex-wrap items-center gap-2 border-b border-border bg-muted/30 px-3 py-2 text-xs">
          <p class="min-w-0 flex-1">{{ t('learn.package.exitBody') }}</p>
          <AppButton data-package-exit-confirm size="xs" @click="request('exitAll')">{{ t('learn.package.exitConfirm') }}</AppButton>
          <AppButton size="xs" variant="ghost" @click="confirmingExit = false">{{ t('learn.package.freshCancel') }}</AppButton>
        </div>

        <div
          v-if="saveError"
          data-package-save-error
          role="alert"
          class="shrink-0 border-b border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-destructive"
        >
          <p>{{ t('learn.package.saveError') }}</p>
          <details class="mt-1">
            <summary class="cursor-pointer">{{ t('learn.package.details') }}</summary>
            <code class="break-all">{{ saveError }}</code>
          </details>
        </div>

        <ul v-if="blocked.length" data-package-blocked class="shrink-0 space-y-1 border-b border-border bg-muted/30 px-3 py-2 text-xs" role="status">
          <li v-for="item in blocked" :key="`${item.kind}:${item.detail ?? ''}`" class="flex items-start gap-2">
            <span class="min-w-0 flex-1">{{ blockedMessage(item) }}</span>
            <button
              type="button"
              class="shrink-0 text-muted-foreground underline"
              @click="blocked = blocked.filter(entry => entry !== item)"
            >
              {{ t('learn.package.dismiss') }}
            </button>
          </li>
        </ul>

        <div class="relative flex min-h-0 flex-1">
          <div v-if="phase === 'loading' || phase === 'preparing'" class="absolute inset-0 z-10 flex flex-col items-center justify-center gap-2 bg-background text-sm text-muted-foreground">
            <AppSpinner size="md" />
            <p v-if="extracting && extracting.files_total > 0">
              {{ t('learn.package.unpacking', { done: extracting.files_done, total: extracting.files_total }) }}
            </p>
            <p v-else>{{ t('learn.package.preparing') }}</p>
          </div>
          <div
            v-else-if="sequenced && phase === 'ready' && sequencedPhase !== 'running'"
            :data-package-sequencing="sequencedPhase"
            role="status"
            class="absolute inset-0 z-10 flex flex-col items-center justify-center gap-3 bg-background p-6 text-center text-sm text-muted-foreground"
          >
            <template v-if="sequencedPhase === 'choose'">
              <p>{{ t('learn.package.choose') }}</p>
            </template>
            <template v-else-if="endedReason === 'suspend_all'">
              <p>{{ t('learn.package.suspended') }}</p>
              <AppButton data-package-resume size="sm" @click="request('resumeAll')">{{ t('learn.package.resume') }}</AppButton>
            </template>
            <p v-else>{{ t('learn.package.ended') }}</p>
          </div>
          <div
            v-if="unitWindow"
            data-package-unit-window
            class="absolute inset-0 z-10 flex flex-col items-center justify-center gap-3 bg-background p-6 text-center text-sm text-muted-foreground"
            role="status"
          >
            <p>{{ t('learn.package.unitWindow.body') }}</p>
            <div class="flex flex-wrap justify-center gap-2">
              <AppButton data-package-unit-focus size="xs" @click="focusUnitWindow">{{ t('learn.package.unitWindow.focus') }}</AppButton>
              <AppButton data-package-unit-end size="xs" variant="secondary" @click="endUnit">{{ t('learn.package.unitWindow.end') }}</AppButton>
            </div>
          </div>
          <p
            v-else-if="unitExited"
            data-package-unit-exited
            class="absolute inset-x-0 top-0 z-10 border-b border-border bg-muted/60 px-3 py-2 text-center text-xs text-muted-foreground"
            role="status"
          >
            {{ t('learn.package.unitExited') }}
          </p>
          <iframe
            v-if="phase !== 'loading'"
            :key="frameKey"
            ref="frame"
            data-package-frame
            :title="t('learn.package.frameTitle', { title: element.title })"
            :sandbox="PACKAGE_FRAME_SANDBOX"
            allow=""
            referrerpolicy="no-referrer"
            class="h-full min-h-[24rem] w-full flex-1 border-0 bg-white"
          />
        </div>
      </div>
    </div>
  </div>
</template>
