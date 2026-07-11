<script setup lang="ts">
import { ref, onMounted, onUnmounted, computed, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'
import { useLocalApi } from '@/composables/useLocalApi'
import { useSentinel } from '@/composables/useSentinel'
import { AppButton, ProvenanceBadge } from '@/components/ui'
import InfoTip from '@/components/ui/InfoTip.vue'
import { resolveElementBinding, type ElementHostContext } from '@/components/course/elementRegistry'
import { useCourseCompletion } from '@/composables/useCourseCompletion'
import type { Course, Chapter, Element, Enrollment, ElementProgress, UpdateProgressRequest, QuizResult } from '@/types'

const { invoke } = useLocalApi()
const { t } = useI18n()
const route = useRoute()
const router = useRouter()
const courseCompletion = useCourseCompletion()

const sentinel = useSentinel()

const courseId = route.params.id as string

const course = ref<Course | null>(null)
const chapters = ref<Chapter[]>([])
const elements = ref<Record<string, Element[]>>({})
const enrollment = ref<Enrollment | null>(null)
const progress = ref<Record<string, ElementProgress>>({})
const loading = ref(true)
const enrolling = ref(false)

// Auto-earn completion claim
interface UnmetElement {
  element_id: string
  title: string
  element_type: string
  best_score: number | null
  required_score: number
}
interface CourseCompletionStatus {
  ready: boolean
  missing_elements: string[]
  required_count: number
  preview: { leaves: string[]; root: string } | null
  unmet_elements: UnmetElement[]
}
const completionStatus = ref<CourseCompletionStatus | null>(null)
const claiming = ref(false)
const claimError = ref<string | null>(null)
const claimTxHash = ref<string | null>(null)
const claimCredentialIds = ref<string[]>([])

async function refreshCompletionStatus() {
  try {
    completionStatus.value = await invoke<CourseCompletionStatus>('get_course_completion_status', { courseId })
  } catch (e) {
    console.error('completion status failed:', e)
  }
}

async function claimCredential() {
  claiming.value = true
  claimError.value = null
  try {
    const result = await invoke<{ tx_hash: string; credential_ids: string[] }>(
      'claim_course_completion',
      { courseId, timestampMs: Date.now() },
    )
    claimTxHash.value = result.tx_hash
    claimCredentialIds.value = result.credential_ids ?? []
  } catch (e) {
    claimError.value = String(e)
  } finally {
    claiming.value = false
  }
}

// Set once we've minted (auto or manual) for this session so the auto-mint
// trigger in markComplete fires at most once per visit.
const autoMintFired = ref(false)

// Claim the completion credentials and raise the celebration modal. Skills are
// minted by default at the end of a course/tutorial, so this runs automatically
// when the final element completes (see markComplete) — the "Get completion
// credential" button and "Finish Course" reuse it. Best-effort: never block on
// it. claim_course_completion always issues local credentials (even for
// content-only courses); on-chain anchoring is an upgrade, not required. We do
// NOT navigate away — the modal's "View credential" / "Continue" own that.
async function mintAndCelebrate() {
  autoMintFired.value = true
  await refreshCompletionStatus()
  claimTxHash.value = null
  claimCredentialIds.value = []
  await claimCredential().catch(() => {})

  courseCompletion.open({
    courseTitle: course.value?.title ?? t('learn.player.courseFallback'),
    courseId,
    skillIds: course.value?.skill_ids ?? [],
    txHash: claimTxHash.value,
    credentialIds: claimCredentialIds.value,
    isTutorial: isTutorial.value,
    unmetElements: completionStatus.value?.unmet_elements ?? [],
  })
}

// "Finish Course" on the last element: mark the final content element
// complete (so the course can qualify), then mint. (markComplete also
// auto-mints, but a content element the user hasn't explicitly completed
// needs the mark first.)
async function finishCourse() {
  if (
    enrollment.value &&
    isContentElement.value &&
    currentElement.value &&
    elementStatus(currentElement.value.id) !== 'completed'
  ) {
    await markComplete()
  }
  // markComplete auto-mints when it completes the final element; only mint
  // here if it didn't (e.g. the course was already fully complete).
  if (!autoMintFired.value) await mintAndCelebrate()
}

const sentinelStarted = ref(false)
const downloadingElementId = ref<string | null>(null)
const downloadError = ref<string | null>(null)

// Camera verification — see docs/sentinel.md §Camera. Opt-in only; all
// processing is on-device, no frames stored or sent. Two cadences:
//   - gaze (second-device / look-away): fast, to catch brief glances.
//   - face identity (LBP): slower; identity doesn't change tick-to-tick
//     and the LBP pass is synchronous, so we run it less often.
const GAZE_LOOP_INTERVAL_MS = 1000
const FACE_ID_LOOP_INTERVAL_MS = 3000
const cameraVideoRef = ref<HTMLVideoElement | null>(null)
const cameraStream = ref<MediaStream | null>(null)
const cameraError = ref<string | null>(null)
const cameraStarting = ref(false)
const lastFacePresent = ref<boolean | null>(null)
let gazeLoopTimer: ReturnType<typeof setInterval> | null = null
let faceIdLoopTimer: ReturnType<typeof setInterval> | null = null
let gazeInFlight = false

const activeChapter = ref<string | null>(null)
const activeElement = ref<string | null>(null)

const currentElement = computed(() => {
  if (!activeChapter.value || !activeElement.value) return null
  return elements.value[activeChapter.value]?.find(e => e.id === activeElement.value) ?? null
})

const currentChapter = computed(() => {
  if (!activeChapter.value) return null
  return chapters.value.find(c => c.id === activeChapter.value) ?? null
})

type FlatCourseElement = {
  chapterId: string
  chapterTitle: string
  element: Element
}

const flatElements = computed<FlatCourseElement[]>(() => {
  const out: FlatCourseElement[] = []
  for (const ch of chapters.value) {
    for (const el of elements.value[ch.id] ?? []) {
      out.push({ chapterId: ch.id, chapterTitle: ch.title, element: el })
    }
  }
  return out
})

const activeFlatIndex = computed(() => {
  if (!activeElement.value) return -1
  return flatElements.value.findIndex(item => item.element.id === activeElement.value)
})

const hasPrevElement = computed(() => activeFlatIndex.value > 0)
const hasNextElement = computed(() => activeFlatIndex.value >= 0 && activeFlatIndex.value < flatElements.value.length - 1)

const isAssessment = computed(() => {
  if (!currentElement.value) return false
  return isAssessmentElement(currentElement.value.element_type)
})

// Whether the current element type supports "Mark Complete" manually
const isContentElement = computed(() => {
  if (!currentElement.value) return false
  const t = currentElement.value.element_type
  return t === 'video' || t === 'text' || t === 'pdf' || t === 'downloadable' || t === 'interactive'
})

// Once the enrollment is completed, assessment responses are locked: the
// learner can review their past answers but not re-submit them.
const courseCompleted = computed(() => enrollment.value?.status === 'completed')

// Standalone tutorial: hide chapter sidebar entirely — tutorials have a
// single video element, the chapter nav is pure noise.
const isTutorial = computed(() => course.value?.kind === 'tutorial')

// Widen the content column for video so large displays don't leave
// huge dead margins around the player. Other element types stay at the
// readable text width.
const isVideoElement = computed(() => currentElement.value?.element_type === 'video')

// Check if an element type is MCQ
function isMcqType(type: string): boolean {
  return type === 'objective_single_mcq' || type === 'objective_multi_mcq' || type === 'subjective_mcq'
}

// Check if assessment element (Sentinel activates for these)
function isAssessmentElement(type: string): boolean {
  return isMcqType(type) || type === 'essay' || type === 'quiz' || type === 'assessment' || type === 'interactive'
}

// Total progress stats
const totalElements = computed(() => {
  let count = 0
  for (const chElems of Object.values(elements.value)) {
    count += chElems.length
  }
  return count
})

const completedElements = computed(() => {
  let count = 0
  for (const p of Object.values(progress.value)) {
    if (p.status === 'completed') count++
  }
  return count
})

const progressPercent = computed(() => {
  if (totalElements.value === 0) return 0
  return Math.round((completedElements.value / totalElements.value) * 100)
})

// You can only finish once the course is actually complete: every element
// done, or just the current last *content* element outstanding (which Finish
// itself marks complete). A still-incomplete element (incl. an unpassed
// assessment, which stays non-'completed') keeps Finish disabled.
const canFinish = computed(() => {
  const total = totalElements.value
  if (total === 0) return false
  const done = completedElements.value
  if (done >= total) return true
  return done === total - 1 && isLastElement.value && isContentElement.value
})

// Check if current element is the very last in the course
const isLastElement = computed(() => {
  if (!activeChapter.value || !activeElement.value) return false
  const lastCh = chapters.value[chapters.value.length - 1]
  if (!lastCh || lastCh.id !== activeChapter.value) return false
  const chElems = elements.value[lastCh.id]
  if (!chElems || chElems.length === 0) return false
  return chElems[chElems.length - 1]?.id === activeElement.value
})

// Skill tags for current element (from Element.skills if available)
const elementSkills = computed(() => {
  if (!currentElement.value) return []
  return (currentElement.value as any).skills ?? []
})

onMounted(async () => {
  window.addEventListener('keydown', onGlobalKeydown)
  try {
    const [c, chs, enrollments] = await Promise.all([
      invoke<Course>('get_course', { courseId }),
      invoke<Chapter[]>('list_chapters', { courseId }),
      invoke<Enrollment[]>('list_enrollments'),
    ])
    course.value = c
    chapters.value = chs
    enrollment.value = enrollments.find(e => e.course_id === courseId) ?? null

    // Load elements
    for (const ch of chs) {
      elements.value[ch.id] = await invoke<Element[]>('list_elements', { chapterId: ch.id }).catch(() => [])
    }

    // Tutorials auto-enroll silently so progress tracks without the
    // user having to click "Enroll". Clicking a tutorial on the home
    // page goes straight to the player — no detail/enrollment gate.
    if (!enrollment.value && c.kind === 'tutorial') {
      try {
        enrollment.value = await invoke<Enrollment>('enroll', { courseId })
      } catch (e) {
        console.warn('Tutorial auto-enroll failed (non-blocking):', e)
      }
    }

    // Load progress if enrolled
    if (enrollment.value) {
      try {
        const p = await invoke<ElementProgress[]>('get_progress', { enrollmentId: enrollment.value.id })
        for (const ep of p) {
          progress.value[ep.element_id] = ep
        }
      } catch { /* no progress yet */ }

      // Start Sentinel monitoring for this enrollment
      await sentinel.start(enrollment.value.id)
      sentinelStarted.value = true

      await refreshCompletionStatus()
    }

    // Select resume point: first incomplete element, fallback to first element
    const flattened: FlatCourseElement[] = []
    for (const ch of chs) {
      for (const el of elements.value[ch.id] ?? []) {
        flattened.push({ chapterId: ch.id, chapterTitle: ch.title, element: el })
      }
    }

    const firstIncomplete = flattened.find(item => (progress.value[item.element.id]?.status ?? 'not_started') !== 'completed')
    const firstItem = firstIncomplete ?? flattened[0]
    if (firstItem) {
      activeChapter.value = firstItem.chapterId
      activeElement.value = firstItem.element.id
    }
  } catch (e) {
    console.error('Failed to load course:', e)
  } finally {
    loading.value = false
  }
})

// Notify Sentinel when element changes
watch([activeChapter, activeElement], () => {
  if (currentElement.value && sentinelStarted.value) {
    sentinel.setElement(currentElement.value.id, currentElement.value.element_type)
  }
  downloadError.value = null
  void markInProgress()
})

onUnmounted(async () => {
  window.removeEventListener('keydown', onGlobalKeydown)
  stopFaceLoop()
  releaseCameraStream()
  if (sentinelStarted.value) {
    await sentinel.stop()
  }
})

async function enableCamera() {
  if (cameraStream.value || cameraStarting.value) return
  cameraError.value = null
  cameraStarting.value = true
  try {
    const stream = await navigator.mediaDevices.getUserMedia({
      video: { width: 320, height: 240, facingMode: 'user' },
      audio: false,
    })
    cameraStream.value = stream
    // Wait a tick so the <video> element is rendered under v-if before attach.
    await new Promise(resolve => setTimeout(resolve, 0))
    const video = cameraVideoRef.value
    if (video) {
      video.srcObject = stream
      video.muted = true
      await video.play().catch(() => { /* autoplay rules — loop will retry */ })
    }
    sentinel.setCameraOptedIn(true)
    startFaceLoop()
  } catch (e) {
    cameraError.value = e instanceof Error ? e.message : t('learn.player.cameraUnavailable')
    releaseCameraStream()
  } finally {
    cameraStarting.value = false
  }
}

function disableCamera() {
  stopFaceLoop()
  releaseCameraStream()
  sentinel.setCameraOptedIn(false)
  lastFacePresent.value = null
}

function releaseCameraStream() {
  if (cameraStream.value) {
    for (const track of cameraStream.value.getTracks()) track.stop()
    cameraStream.value = null
  }
  if (cameraVideoRef.value) cameraVideoRef.value.srcObject = null
}

function startFaceLoop() {
  if (gazeLoopTimer || faceIdLoopTimer) return
  // Fast gaze loop — catches brief look-aways. Backend YuNet + head-pose;
  // an in-flight guard prevents pile-up if a tick is slower than the
  // interval (e.g. on mobile).
  gazeLoopTimer = setInterval(() => {
    const video = cameraVideoRef.value
    if (!video || gazeInFlight) return
    gazeInFlight = true
    void sentinel.scoreGaze(video).finally(() => { gazeInFlight = false })
  }, GAZE_LOOP_INTERVAL_MS)
  // Slower LBP identity/presence loop (synchronous, advisory).
  faceIdLoopTimer = setInterval(() => {
    const video = cameraVideoRef.value
    if (!video) return
    lastFacePresent.value = sentinel.verifyFace(video).present
  }, FACE_ID_LOOP_INTERVAL_MS)
}

function stopFaceLoop() {
  if (gazeLoopTimer) { clearInterval(gazeLoopTimer); gazeLoopTimer = null }
  if (faceIdLoopTimer) { clearInterval(faceIdLoopTimer); faceIdLoopTimer = null }
  gazeInFlight = false
}

function selectElement(chapterId: string, elementId: string) {
  activeChapter.value = chapterId
  activeElement.value = elementId
}

// --- Mobile chapter navigator (bottom sheet) ---
const mobileNavOpen = ref(false)
const expandedChapters = ref<Set<string>>(new Set())

function openMobileNav() {
  // Expand the chapter the learner is currently in so it's visible on open.
  expandedChapters.value = new Set(activeChapter.value ? [activeChapter.value] : [])
  mobileNavOpen.value = true
}

function toggleChapterExpanded(chapterId: string) {
  const next = new Set(expandedChapters.value)
  if (next.has(chapterId)) next.delete(chapterId)
  else next.add(chapterId)
  expandedChapters.value = next
}

function selectFromMobileNav(chapterId: string, elementId: string) {
  selectElement(chapterId, elementId)
  mobileNavOpen.value = false
}

async function enrollFromPlayer() {
  if (!course.value || enrollment.value) return
  enrolling.value = true
  try {
    enrollment.value = await invoke<Enrollment>('enroll', { courseId: course.value.id })
    if (enrollment.value && !sentinelStarted.value) {
      await sentinel.start(enrollment.value.id)
      sentinelStarted.value = true
      if (currentElement.value) {
        sentinel.setElement(currentElement.value.id, currentElement.value.element_type)
      }
    }
  } catch (e) {
    console.error('Failed to enroll from player:', e)
  } finally {
    enrolling.value = false
  }
}

async function markInProgress() {
  if (!enrollment.value || !activeElement.value) return
  const current = progress.value[activeElement.value]
  if (current?.status === 'completed' || current?.status === 'in_progress') return
  try {
    const req: UpdateProgressRequest = {
      element_id: activeElement.value,
      status: 'in_progress',
      score: current?.score ?? null,
    }
    await invoke('update_progress', {
      enrollmentId: enrollment.value.id,
      req,
    })
    progress.value[activeElement.value] = {
      ...current,
      id: current?.id ?? '',
      enrollment_id: enrollment.value.id,
      element_id: activeElement.value,
      status: 'in_progress',
      score: current?.score ?? null,
      time_spent: current?.time_spent ?? 0,
      completed_at: current?.completed_at ?? null,
      updated_at: new Date().toISOString(),
    }
  } catch (e) {
    console.error('Failed to mark in progress:', e)
  }
}

async function markComplete(score?: number) {
  if (!enrollment.value || !activeElement.value) return
  try {
    const req: UpdateProgressRequest = {
      element_id: activeElement.value,
      status: 'completed',
      score: score ?? null,
    }
    await invoke('update_progress', {
      enrollmentId: enrollment.value.id,
      req,
    })
    // Update local progress
    progress.value[activeElement.value] = {
      ...progress.value[activeElement.value],
      id: progress.value[activeElement.value]?.id ?? '',
      enrollment_id: enrollment.value.id,
      element_id: activeElement.value,
      status: 'completed',
      score: score ?? null,
      time_spent: 0,
      completed_at: new Date().toISOString(),
      updated_at: new Date().toISOString(),
    }
    // A newly-passed assessment may complete the course — refresh the
    // claim affordance.
    void refreshCompletionStatus()
    // Skills mint by default at the end of a course/tutorial: when this
    // completes the final element, auto-claim the completion credentials and
    // celebrate. Guarded so it fires at most once and never on a course that
    // was already complete when opened.
    if (
      totalElements.value > 0 &&
      completedElements.value === totalElements.value &&
      !autoMintFired.value &&
      !courseCompleted.value
    ) {
      void mintAndCelebrate()
      return
    }
    // Auto-advance to next element after a short delay — but skip for
    // element types where staying put is more useful (replay results,
    // try again, review the score) than jumping ahead.
    if (shouldAutoAdvance(currentElement.value?.element_type)) {
      setTimeout(() => advanceToNext(), 500)
    }
  } catch (e) {
    console.error('Failed to update progress:', e)
  }
}

function sanitizeFileName(name: string): string {
  const trimmed = name.trim()
  const safe = trimmed.replace(/[\\/:*?"<>|]/g, '-').replace(/\s+/g, ' ')
  return safe || 'download'
}

function inferMimeFromName(fileName: string | null | undefined): string | null {
  if (!fileName) return null
  const lower = fileName.toLowerCase()
  if (lower.endsWith('.pdf')) return 'application/pdf'
  if (lower.endsWith('.txt')) return 'text/plain'
  if (lower.endsWith('.zip')) return 'application/zip'
  if (lower.endsWith('.json')) return 'application/json'
  if (lower.endsWith('.csv')) return 'text/csv'
  return null
}

function extensionForMime(mime: string): string {
  if (mime === 'application/pdf') return 'pdf'
  if (mime === 'text/plain') return 'txt'
  if (mime === 'application/zip') return 'zip'
  if (mime === 'application/json') return 'json'
  if (mime === 'text/csv') return 'csv'
  return 'bin'
}

function buildDownloadFileName(rawName: string | null | undefined, mimeType: string): string {
  const base = sanitizeFileName(rawName || 'download')
  if (/\.[a-z0-9]+$/i.test(base)) return base
  return `${base}.${extensionForMime(mimeType)}`
}

async function onDownloadClick() {
  if (!currentElement.value) return
  const element = currentElement.value as any
  downloadError.value = null

  if (!element.content_cid) {
    downloadError.value = t('learn.player.noFileAttached')
    return
  }

  downloadingElementId.value = currentElement.value.id

  try {
    const bytes = await invoke<number[]>('content_resolve_bytes', {
      identifier: element.content_cid,
    })

    const mimeType = element.mime_type || inferMimeFromName(element.filename) || 'application/octet-stream'
    const fileName = buildDownloadFileName(element.filename || element.title, mimeType)
    const blob = new Blob([new Uint8Array(bytes)], { type: mimeType })
    const objectUrl = URL.createObjectURL(blob)

    const anchor = document.createElement('a')
    anchor.href = objectUrl
    anchor.download = fileName
    document.body.appendChild(anchor)
    anchor.click()
    anchor.remove()
    setTimeout(() => URL.revokeObjectURL(objectUrl), 1500)

    await markComplete()
  } catch (e) {
    console.error('Failed to download content:', e)
    downloadError.value = t('learn.player.downloadFailed', { error: String(e) })
  } finally {
    downloadingElementId.value = null
  }
}

/** Element types that should NOT auto-advance on `markComplete`. Plugin
 *  elements are interactive (replay, retry, review results), so jumping
 *  forward on completion is almost always wrong. The learner clicks Next
 *  themselves. Future: plugin manifest can opt back into auto-advance. */
function shouldAutoAdvance(elementType: string | undefined): boolean {
  if (!elementType) return true
  const NO_AUTO_ADVANCE: ReadonlyArray<string> = ['plugin']
  return !NO_AUTO_ADVANCE.includes(elementType)
}

function advanceToNext() {
  if (!activeChapter.value || !activeElement.value) return
  const chElems = elements.value[activeChapter.value]
  if (!chElems) return
  const idx = chElems.findIndex(e => e.id === activeElement.value)
  if (idx >= 0 && idx < chElems.length - 1) {
    const nextEl = chElems[idx + 1]
    if (nextEl) {
      activeElement.value = nextEl.id
      return
    }
  }
  const chIdx = chapters.value.findIndex(c => c.id === activeChapter.value)
  if (chIdx >= 0 && chIdx < chapters.value.length - 1) {
    const nextCh = chapters.value[chIdx + 1]
    if (!nextCh) return
    const nextElems = elements.value[nextCh.id]
    if (nextElems && nextElems.length > 0) {
      activeChapter.value = nextCh.id
      activeElement.value = nextElems[0]!.id
    }
  }
}

function goToNext() {
  if (!hasNextElement.value) return
  advanceToNext()
}

function goToPrev() {
  if (!activeChapter.value || !activeElement.value) return
  const chElems = elements.value[activeChapter.value]
  if (!chElems) return
  const idx = chElems.findIndex(e => e.id === activeElement.value)
  if (idx > 0) {
    activeElement.value = chElems[idx - 1]!.id
    return
  }
  // Go to last element of previous chapter
  const chIdx = chapters.value.findIndex(c => c.id === activeChapter.value)
  if (chIdx > 0) {
    const prevCh = chapters.value[chIdx - 1]
    if (!prevCh) return
    const prevElems = elements.value[prevCh.id]
    if (prevElems && prevElems.length > 0) {
      activeChapter.value = prevCh.id
      activeElement.value = prevElems[prevElems.length - 1]!.id
    }
  }
}

function onGlobalKeydown(event: KeyboardEvent) {
  if (event.key === 'Escape' && mobileNavOpen.value) {
    mobileNavOpen.value = false
    return
  }
  if (event.key === 'ArrowRight') {
    goToNext()
  }
  if (event.key === 'ArrowLeft') {
    goToPrev()
  }
}

function elementStatus(elementId: string): string {
  return progress.value[elementId]?.status ?? 'not_started'
}

function elementTypeIcon(elementType: string): string {
  switch (elementType) {
    case 'video': return 'M14.752 11.168l-3.197-2.132A1 1 0 0010 9.87v4.263a1 1 0 001.555.832l3.197-2.132a1 1 0 000-1.664z'
    case 'text': return 'M4 6h16M4 12h16M4 18h7'
    case 'pdf': return 'M7 21h10a2 2 0 002-2V9.414a1 1 0 00-.293-.707l-5.414-5.414A1 1 0 0012.586 3H7a2 2 0 00-2 2v14a2 2 0 002 2z'
    case 'downloadable': return 'M4 16v1a3 3 0 003 3h10a3 3 0 003-3v-1m-4-4l-4 4m0 0l-4-4m4 4V4'
    case 'quiz': case 'assessment': return 'M9 5H7a2 2 0 00-2 2v12a2 2 0 002 2h10a2 2 0 002-2V7a2 2 0 00-2-2h-2M9 5a2 2 0 002 2h2a2 2 0 002-2M9 5a2 2 0 012-2h2a2 2 0 012 2'
    case 'objective_single_mcq': case 'objective_multi_mcq': case 'subjective_mcq': return 'M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z'
    case 'essay': return 'M11 5H6a2 2 0 00-2 2v11a2 2 0 002 2h11a2 2 0 002-2v-5m-1.414-9.414a2 2 0 112.828 2.828L11.828 15H9v-2.828l8.586-8.586z'
    case 'interactive': return 'M13 10V3L4 14h7v7l9-11h-7z'
    default: return 'M7 21h10a2 2 0 002-2V9.414a1 1 0 00-.293-.707l-5.414-5.414A1 1 0 0012.586 3H7a2 2 0 00-2 2v14a2 2 0 002 2z'
  }
}

function elementTypeLabel(elementType: string): string {
  switch (elementType) {
    case 'video': return t('learn.player.elementType.video')
    case 'text': return t('learn.player.elementType.text')
    case 'pdf': return t('learn.player.elementType.pdf')
    case 'downloadable': return t('learn.player.elementType.downloadable')
    case 'quiz': return t('learn.player.elementType.quiz')
    case 'assessment': return t('learn.player.elementType.assessment')
    case 'objective_single_mcq': return t('learn.player.elementType.singleChoice')
    case 'objective_multi_mcq': return t('learn.player.elementType.multipleChoice')
    case 'subjective_mcq': return t('learn.player.elementType.subjective')
    case 'essay': return t('learn.player.elementType.essay')
    case 'interactive': return t('learn.player.elementType.interactive')
    default: return t('learn.player.elementType.content')
  }
}

// Element dispatch — see alexandria/src/components/course/elementRegistry.ts
const elementBinding = computed(() =>
  currentElement.value ? resolveElementBinding(currentElement.value.element_type) : null,
)

const elementHostContext = computed<ElementHostContext | null>(() => {
  const el = currentElement.value
  if (!el) return null
  return {
    element: el,
    isCompleted: elementStatus(el.id) === 'completed',
    downloading: downloadingElementId.value === el.id,
    downloadError: downloadError.value,
    enrollmentId: enrollment.value?.id ?? null,
    readOnly: courseCompleted.value,
    onDownload: onDownloadClick,
    onComplete: () => { void markComplete() },
    onScoredComplete: (score: number) => { void markComplete(score) },
    onQuizComplete: (result: QuizResult) => { void markComplete(result.score) },
    elementTypeLabel,
  }
})

</script>

<template>
  <div class="flex-1 min-h-0 flex flex-col">
    <!-- Loading skeleton -->
    <div v-if="loading" class="flex-1 min-h-0 flex gap-0">
      <!-- Sidebar skeleton — hidden on mobile -->
      <div class="hidden md:block w-72 shrink-0 border-r border-border p-4 space-y-4">
        <div class="h-4 w-24 animate-pulse rounded bg-muted/40" />
        <div class="space-y-1">
          <div class="h-5 w-48 animate-pulse rounded bg-muted/40" />
          <div class="h-1.5 w-full animate-pulse rounded-full bg-muted/30" />
        </div>
        <div class="space-y-3 pt-2">
          <div v-for="i in 3" :key="i" class="space-y-1">
            <div class="h-3 w-20 animate-pulse rounded bg-muted/30" />
            <div v-for="j in 3" :key="j" class="flex items-center gap-2 px-2 py-1.5">
              <div class="h-5 w-5 animate-pulse rounded-full bg-muted/30" />
              <div class="h-3.5 flex-1 animate-pulse rounded bg-muted/30" />
            </div>
          </div>
        </div>
      </div>
      <!-- Content skeleton -->
      <div class="flex-1 p-4 md:p-6">
        <div class="max-w-3xl mx-auto space-y-4">
          <div class="flex items-center gap-2">
            <div class="h-5 w-16 animate-pulse rounded-full bg-muted/30" />
            <div class="h-5 w-12 animate-pulse rounded bg-muted/30" />
          </div>
          <div class="h-7 w-64 max-w-full animate-pulse rounded bg-muted/40" />
          <div class="h-[400px] animate-pulse rounded-lg bg-muted/20" />
        </div>
      </div>
    </div>

    <!-- Course not found -->
    <div v-else-if="!course" class="flex items-center justify-center flex-1 min-h-0 px-4">
      <div class="text-center">
        <div class="mx-auto mb-4 flex h-16 w-16 items-center justify-center rounded-full bg-muted/30">
          <svg class="h-8 w-8 text-muted-foreground/50" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
            <path stroke-linecap="round" stroke-linejoin="round" d="M12 6.253v13m0-13C10.832 5.477 9.246 5 7.5 5S4.168 5.477 3 6.253v13C4.168 18.477 5.754 18 7.5 18s3.332.477 4.5 1.253m0-13C13.168 5.477 14.754 5 16.5 5c1.747 0 3.332.477 4.5 1.253v13C19.832 18.477 18.247 18 16.5 18c-1.746 0-3.332.477-4.5 1.253" />
          </svg>
        </div>
        <h2 class="text-lg font-semibold text-foreground">{{ $t('learn.player.notFoundTitle') }}</h2>
        <p class="mt-1 text-sm text-muted-foreground">{{ $t('learn.player.notFoundBody') }}</p>
        <AppButton variant="secondary" size="sm" class="mt-4" @click="router.push('/courses')">
          {{ $t('learn.player.browseCourses') }}
        </AppButton>
      </div>
    </div>

    <!-- Main Player Layout -->
    <div v-else class="flex flex-col md:flex-row gap-0 flex-1 min-h-0">
      <!-- ======================================= -->
      <!-- MOBILE: Compact chapter/element header  -->
      <!-- ======================================= -->
      <div v-if="!isTutorial" class="md:hidden shrink-0 border-b border-border bg-card/70 backdrop-blur">
        <div class="flex items-center gap-2 px-3 py-2">
          <button
            class="p-1 rounded-md text-muted-foreground active:bg-muted"
            :aria-label="$t('learn.player.backToCourse')"
            @click="router.push(`/courses/${courseId}`)"
          >
            <svg class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
              <path stroke-linecap="round" stroke-linejoin="round" d="M15 19l-7-7 7-7" />
            </svg>
          </button>

          <!-- Opens the chapter navigator sheet -->
          <button
            class="flex min-w-0 flex-1 items-center gap-2 rounded-lg px-2 py-1 text-left active:bg-muted"
            :aria-label="$t('learn.player.openChapterNav')"
            @click="openMobileNav"
          >
            <svg class="h-4 w-4 shrink-0 text-muted-foreground" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
              <path stroke-linecap="round" stroke-linejoin="round" d="M4 6h16M4 12h16M4 18h16" />
            </svg>
            <span class="min-w-0 flex-1">
              <span class="block truncate text-[10px] uppercase tracking-wide text-muted-foreground">{{ currentChapter?.title }}</span>
              <span class="block truncate text-xs font-medium text-foreground">{{ currentElement?.title }}</span>
            </span>
            <svg class="h-4 w-4 shrink-0 text-muted-foreground" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
              <path stroke-linecap="round" stroke-linejoin="round" d="M19 9l-7 7-7-7" />
            </svg>
          </button>

          <span class="shrink-0 text-[10px] text-muted-foreground">
            {{ completedElements }}/{{ totalElements }}
          </span>
        </div>
        <div class="h-0.5 bg-muted/40">
          <div
            class="progress-fill h-full"
            :class="progressPercent === 100 ? 'bg-emerald-500' : 'bg-primary'"
            :style="{ width: `${progressPercent}%` }"
          />
        </div>
      </div>

      <!-- ============================== -->
      <!-- SIDEBAR: Chapter/Element Nav   -->
      <!-- ============================== -->
      <div v-if="!isTutorial" class="hidden md:block w-80 shrink-0 overflow-y-auto border-r border-border bg-card/30">
        <div class="p-4 space-y-4">
          <!-- Back link -->
          <button
            class="flex items-center gap-1.5 text-xs text-muted-foreground transition-colors hover:text-foreground"
            @click="router.push(`/courses/${courseId}`)"
          >
            <svg class="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
              <path stroke-linecap="round" stroke-linejoin="round" d="M15 19l-7-7 7-7" />
            </svg>
            {{ $t('learn.player.backToCourse') }}
          </button>

          <!-- Course title + progress -->
          <div class="space-y-2">
            <div class="flex items-start justify-between gap-2">
              <h2 class="text-sm font-semibold text-foreground leading-snug">{{ course.title }}</h2>
              <ProvenanceBadge :provenance="course.provenance" />
            </div>
            <div class="space-y-1">
              <div class="flex items-center justify-between text-xs text-muted-foreground">
                <span>{{ $t('learn.player.completeOfTotal', { done: completedElements, total: totalElements }) }}</span>
                <span class="font-medium">{{ progressPercent }}%</span>
              </div>
              <div class="h-1.5 overflow-hidden rounded-full bg-muted/30">
                <div
                  class="progress-fill h-full rounded-full"
                  :class="progressPercent === 100 ? 'bg-emerald-500' : 'bg-primary'"
                  :style="{ width: `${progressPercent}%` }"
                />
              </div>
            </div>

            <!-- Claim credential — visible once the assessed elements pass -->
            <div v-if="completionStatus?.ready" class="rounded-lg border border-emerald-500/30 bg-emerald-500/10 p-3">
              <p class="text-xs font-medium text-emerald-700 dark:text-emerald-300">
                {{ $t('learn.player.claimReadyTitle') }}
              </p>
              <button
                :disabled="claiming"
                class="mt-2 w-full rounded-md bg-emerald-600 px-3 py-1.5 text-xs font-medium text-white transition-colors hover:bg-emerald-700 disabled:opacity-60"
                @click="claimCredential"
              >
                {{ claiming ? $t('learn.player.claiming') : $t('learn.player.claimCredential') }}
              </button>
              <p v-if="claimError" class="mt-2 text-xs text-red-600 dark:text-red-400">{{ claimError }}</p>
              <div v-if="claimTxHash" class="mt-2 text-xs text-emerald-700 dark:text-emerald-300">
                <p>{{ $t('learn.player.publicRecordSaved') }}</p>
                <details class="mt-1">
                  <summary class="cursor-pointer">{{ $t('common.advanced.toggle') }}</summary>
                  <span class="mt-1 block break-all font-mono">{{ claimTxHash.slice(0, 16) }}…</span>
                </details>
              </div>
            </div>
            <div
              v-else-if="completionStatus && completionStatus.required_count > 0"
              class="text-xs text-muted-foreground"
            >
              {{ $t('learn.player.assessmentsPassed', { passed: completionStatus.required_count - completionStatus.missing_elements.length, total: completionStatus.required_count }) }}
            </div>
            <!-- Content-only / already-finished course: no gradeable gate, so
                 offer the completion credential once everything is consumed.
                 Re-claiming is idempotent, so this also re-mints for a course
                 finished before credentials existed. -->
            <div
              v-else-if="progressPercent === 100 || courseCompleted"
              class="rounded-lg border border-emerald-500/30 bg-emerald-500/10 p-3"
            >
              <p class="text-xs font-medium text-emerald-700 dark:text-emerald-300">
                {{ $t('learn.player.getCredentialTitle') }}
              </p>
              <button
                :disabled="claiming"
                class="mt-2 w-full rounded-md bg-emerald-600 px-3 py-1.5 text-xs font-medium text-white transition-colors hover:bg-emerald-700 disabled:opacity-60"
                @click="finishCourse"
              >
                {{ claiming ? $t('learn.player.creatingCredential') : $t('learn.player.getCompletionCredential') }}
              </button>
              <p v-if="claimError" class="mt-2 text-xs text-red-600 dark:text-red-400">{{ claimError }}</p>
            </div>
          </div>

          <!-- Sentinel integrity monitoring (assessments only) -->
          <div
            v-if="sentinelStarted"
            class="space-y-2.5 rounded-lg border border-border/60 bg-card/60 p-3"
          >
            <div class="flex items-center gap-2">
              <span class="relative flex h-2 w-2">
                <span
                  class="absolute inline-flex h-full w-full animate-ping rounded-full opacity-75"
                  :class="sentinel.integrityScore.value > 0.7 ? 'bg-emerald-500' : sentinel.integrityScore.value > 0.4 ? 'bg-amber-400' : 'bg-red-500'"
                />
                <span
                  class="relative inline-flex h-2 w-2 rounded-full"
                  :class="sentinel.integrityScore.value > 0.7 ? 'bg-emerald-500' : sentinel.integrityScore.value > 0.4 ? 'bg-amber-400' : 'bg-red-500'"
                />
              </span>
              <span class="text-xs font-medium text-foreground">{{ $t('learn.player.integrityMonitoring') }}</span>
              <InfoTip :label="$t('learn.player.whyMonitoringLabel')" placement="bottom" class="ml-auto">
                <span class="mb-1 block font-semibold text-foreground">{{ $t('learn.player.whyMonitoringTitle') }}</span>
                {{ $t('learn.player.whyMonitoringBody') }}
                <span class="mt-2 mb-1 block font-semibold text-foreground">{{ $t('learn.player.privacyTitle') }}</span>
                {{ $t('learn.player.privacyBody') }}
              </InfoTip>
            </div>

            <div class="flex items-center justify-between text-xs">
              <span class="text-muted-foreground">{{ $t('learn.player.integrityScore') }}</span>
              <span
                class="font-semibold"
                :class="sentinel.integrityScore.value > 0.7 ? 'text-emerald-600 dark:text-emerald-400' : sentinel.integrityScore.value > 0.4 ? 'text-amber-600 dark:text-amber-400' : 'text-red-600 dark:text-red-400'"
              >
                {{ Math.round(sentinel.integrityScore.value * 100) }}%
              </span>
            </div>

            <!-- Optional camera presence check -->
            <div class="flex items-center justify-between gap-2 border-t border-border/50 pt-2">
              <span class="inline-flex items-center gap-1.5 text-xs text-muted-foreground">
                <template v-if="cameraStream">
                  <span
                    class="h-1.5 w-1.5 rounded-full"
                    :class="lastFacePresent ? 'bg-emerald-500' : 'bg-muted-foreground/50'"
                  />
                  {{ lastFacePresent === null ? $t('learn.player.cameraConnecting') : lastFacePresent ? $t('learn.player.faceVerified') : $t('learn.player.noFaceDetected') }}
                </template>
                <template v-else>{{ $t('learn.player.cameraOff') }}</template>
              </span>
              <button
                class="rounded-md px-2 py-1 text-[11px] font-medium transition-colors disabled:opacity-60"
                :class="cameraStream
                  ? 'text-muted-foreground hover:text-foreground'
                  : 'bg-primary/10 text-primary hover:bg-primary/15'"
                :disabled="cameraStarting"
                @click="cameraStream ? disableCamera() : enableCamera()"
              >
                {{ cameraStarting ? $t('learn.player.cameraStarting') : cameraStream ? $t('learn.player.cameraTurnOff') : $t('learn.player.cameraEnable') }}
              </button>
            </div>
            <p v-if="cameraError" class="text-[11px] text-red-600 dark:text-red-400">{{ cameraError }}</p>

            <!-- Video drives on-device face detection. Rendered off-screen
                 (fixed, far off-viewport) rather than display:none — WKWebView
                 does not paint frames from a display:none video, so a hidden
                 one yields blank frames and "no face detected". -->
            <video
              ref="cameraVideoRef"
              class="pointer-events-none fixed -left-[10000px] top-0 h-[240px] w-[320px] opacity-0"
              playsinline
              autoplay
              muted
              width="320"
              height="240"
            />
          </div>
        </div>

        <!-- Chapter list -->
        <div class="px-2 pb-4">
          <div v-for="(ch, chIndex) in chapters" :key="ch.id" class="mb-4">
            <!-- Chapter header — a quiet section label so the lessons beneath
                 it read as the primary content. -->
            <div class="flex items-center gap-2 px-2 pb-1.5 pt-1">
              <span
                class="flex h-5 w-5 items-center justify-center rounded text-[10px] font-bold"
                :class="(elements[ch.id] ?? []).every(el => elementStatus(el.id) === 'completed')
                  ? 'bg-emerald-100 text-emerald-700 dark:bg-emerald-900/30 dark:text-emerald-400'
                  : 'bg-muted/40 text-muted-foreground'"
              >
                <svg v-if="(elements[ch.id] ?? []).every(el => elementStatus(el.id) === 'completed')" class="h-3 w-3" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2.5">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M5 13l4 4L19 7" />
                </svg>
                <template v-else>{{ chIndex + 1 }}</template>
              </span>
              <span class="text-[11px] font-semibold uppercase tracking-wider text-muted-foreground/80">
                {{ ch.title }}
              </span>
            </div>

            <!-- Element (lesson) buttons — title is the visual anchor. -->
            <button
              v-for="el in elements[ch.id] ?? []"
              :key="el.id"
              class="group flex w-full items-center gap-2.5 rounded-lg px-2 py-2 text-left transition-all duration-200 md:hover:-translate-y-px md:hover:shadow-sm"
              :class="activeElement === el.id
                ? 'bg-primary/10 shadow-sm ring-1 ring-primary/20'
                : 'hover:bg-muted/50'"
              @click="selectElement(ch.id, el.id)"
            >
              <!-- Status/type indicator -->
              <span
                class="flex h-5 w-5 items-center justify-center rounded-full border flex-shrink-0"
                :class="elementStatus(el.id) === 'completed'
                  ? 'border-emerald-500 bg-emerald-500 text-white'
                  : activeElement === el.id
                    ? 'border-primary bg-primary/10'
                    : elementStatus(el.id) === 'in_progress'
                      ? 'border-amber-400 bg-amber-400/10'
                      : 'border-border'"
              >
                <svg v-if="elementStatus(el.id) === 'completed'" class="h-2.5 w-2.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="3">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M5 13l4 4L19 7" />
                </svg>
                <svg v-else class="h-2.5 w-2.5" :class="activeElement === el.id ? '' : 'opacity-50'" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                  <path stroke-linecap="round" stroke-linejoin="round" :d="elementTypeIcon(el.element_type)" />
                </svg>
              </span>
              <span class="min-w-0 flex-1">
                <span
                  class="block truncate text-[13px] font-medium leading-tight"
                  :class="activeElement === el.id
                    ? 'text-primary'
                    : elementStatus(el.id) === 'completed'
                      ? 'text-foreground/70'
                      : 'text-foreground group-hover:text-foreground'"
                >
                  {{ el.title }}
                </span>
                <span class="mt-0.5 block truncate text-[11px] leading-tight text-muted-foreground">
                  {{ elementTypeLabel(el.element_type) }}<template v-if="el.duration_seconds"> · {{ $t('learn.player.minutes', { count: Math.round(el.duration_seconds / 60) }) }}</template>
                </span>
              </span>
            </button>
          </div>
        </div>
      </div>

      <!-- ============================== -->
      <!-- MAIN CONTENT AREA              -->
      <!-- ============================== -->
      <div class="flex-1 flex flex-col overflow-hidden">
        <div v-if="currentElement" :key="currentElement.id" class="lesson-body flex-1 min-h-0 flex flex-col overflow-hidden bg-gradient-to-b from-muted/20 via-transparent to-transparent">
            <!-- Element header -->
            <div class="shrink-0 z-10 border-b border-border/70 bg-background/90 px-4 md:px-6 py-3 backdrop-blur supports-[backdrop-filter]:bg-background/70">
              <!-- Breadcrumb -->
              <div class="flex items-center gap-1.5 text-xs text-muted-foreground mb-3">
                <span>{{ currentChapter?.title }}</span>
                <svg class="h-3 w-3" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M9 5l7 7-7 7" />
                </svg>
                <span class="text-foreground">{{ currentElement.title }}</span>
              </div>

              <!-- Title row -->
              <div class="flex items-start justify-between gap-4">
                <div class="min-w-0">
                  <h1 class="text-xl md:text-2xl font-bold text-foreground leading-tight">{{ currentElement.title }}</h1>
                  <div class="mt-2 flex flex-wrap items-center gap-2">
                    <!-- Element type badge -->
                    <span class="inline-flex items-center gap-1.5 rounded-full px-2.5 py-0.5 text-xs font-medium"
                      :class="{
                        'bg-blue-100 text-blue-700 dark:bg-blue-900/30 dark:text-blue-400': currentElement.element_type === 'video' || isMcqType(currentElement.element_type),
                        'bg-gray-100 text-gray-700 dark:bg-gray-800 dark:text-gray-400': currentElement.element_type === 'text',
                        'bg-red-100 text-red-700 dark:bg-red-900/30 dark:text-red-400': currentElement.element_type === 'pdf',
                        'bg-green-100 text-green-700 dark:bg-green-900/30 dark:text-green-400': currentElement.element_type === 'downloadable',
                        'bg-amber-100 text-amber-700 dark:bg-amber-900/30 dark:text-amber-400': currentElement.element_type === 'quiz' || currentElement.element_type === 'assessment' || currentElement.element_type === 'interactive',
                        'bg-purple-100 text-purple-700 dark:bg-purple-900/30 dark:text-purple-400': currentElement.element_type === 'essay',
                      }"
                    >
                      <svg class="h-3 w-3" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                        <path stroke-linecap="round" stroke-linejoin="round" :d="elementTypeIcon(currentElement.element_type)" />
                      </svg>
                      {{ elementTypeLabel(currentElement.element_type) }}
                    </span>
                    <!-- Duration -->
                    <span v-if="currentElement.duration_seconds" class="text-xs text-muted-foreground">
                      {{ $t('learn.player.minutes', { count: Math.round(currentElement.duration_seconds / 60) }) }}
                    </span>
                    <!-- Monitored badge -->
                    <span v-if="isAssessment" class="inline-flex items-center gap-1 rounded-full bg-amber-100 px-2 py-0.5 text-[10px] font-medium text-amber-700 dark:bg-amber-900/30 dark:text-amber-400">
                      <svg class="h-3 w-3" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                        <path stroke-linecap="round" stroke-linejoin="round" d="M9 12.75L11.25 15 15 9.75m-3-7.036A11.959 11.959 0 013.598 6 11.99 11.99 0 003 9.749c0 5.592 3.824 10.29 9 11.623 5.176-1.332 9-6.03 9-11.622 0-1.31-.21-2.571-.598-3.751h-.152c-3.196 0-6.1-1.248-8.25-3.285z" />
                      </svg>
                      {{ $t('learn.player.monitored') }}
                    </span>
                  </div>
                </div>

                <!-- Status -->
                <div v-if="elementStatus(currentElement.id) === 'completed'" class="flex-shrink-0">
                  <span class="inline-flex items-center gap-1.5 rounded-full bg-emerald-100 px-2.5 py-1 text-xs font-medium text-emerald-700 dark:bg-emerald-900/30 dark:text-emerald-400">
                    <svg class="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                      <path stroke-linecap="round" stroke-linejoin="round" d="M5 13l4 4L19 7" />
                    </svg>
                    {{ $t('learn.player.completed') }}
                    <span v-if="progress[currentElement.id]?.score != null" class="ml-0.5">
                      {{ Math.round((progress[currentElement.id]!.score!) * 100) }}%
                    </span>
                  </span>
                </div>
                <div v-else-if="elementStatus(currentElement.id) === 'in_progress'" class="flex-shrink-0">
                  <span class="inline-flex items-center gap-1 rounded-full bg-amber-100 px-2.5 py-1 text-xs font-medium text-amber-700 dark:bg-amber-900/30 dark:text-amber-400">
                    {{ $t('learn.player.inProgress') }}
                  </span>
                </div>
              </div>

              <div class="mt-3">
                <div class="flex items-center justify-between text-[11px] text-muted-foreground mb-1">
                  <span>{{ $t('learn.player.courseProgress') }}</span>
                  <span>{{ completedElements }} / {{ totalElements }}</span>
                </div>
                <div class="h-1.5 overflow-hidden rounded-full bg-muted/40">
                  <div
                    class="progress-fill h-full rounded-full"
                    :class="progressPercent === 100 ? 'bg-success' : 'bg-primary'"
                    :style="{ width: `${progressPercent}%` }"
                  />
                </div>
              </div>

              <!-- Skill tags -->
              <div v-if="elementSkills.length > 0" class="mt-3 flex flex-wrap gap-1.5">
                <router-link
                  v-for="skill in elementSkills"
                  :key="skill.skill_id || skill.id"
                  :to="`/skills/${skill.skill_id || skill.id}`"
                  class="inline-flex items-center rounded-full bg-primary/8 px-2 py-0.5 text-[10px] font-medium text-primary transition-colors hover:bg-primary/15"
                >
                  {{ skill.skill_name || skill.name }}
                </router-link>
              </div>
            </div>

            <!-- ============================== -->
            <!-- CONTENT RENDERERS              -->
            <!-- Dispatched via elementRegistry. Phase 0 of plugin system. -->
            <!-- ============================== -->
            <div
              class="lesson-content flex-1 min-h-0 flex overflow-y-auto"
              :class="isVideoElement
                ? ''
                : 'px-4 md:px-6 py-4 md:py-6 pb-[calc(1rem+var(--sab,env(safe-area-inset-bottom)))]'"
            >
              <component
                :is="elementBinding!.component"
                v-if="elementBinding && elementHostContext"
                :key="`${currentElement.element_type}-${activeChapter}-${activeElement}`"
                class="flex-1 min-h-0 min-w-0"
                v-bind="elementBinding.props(elementHostContext)"
                v-on="elementBinding.events(elementHostContext)"
              />
            </div>
        </div>

        <!-- Empty state when no element selected -->
        <div v-else class="flex-1 flex items-center justify-center">
          <div class="text-center">
            <div class="mx-auto mb-4 flex h-16 w-16 items-center justify-center rounded-full bg-muted/30">
              <svg class="h-8 w-8 text-muted-foreground/50" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
                <path stroke-linecap="round" stroke-linejoin="round" d="M4 6h16M4 12h16M4 18h7" />
              </svg>
            </div>
            <h3 class="text-sm font-medium text-foreground">{{ $t('learn.player.noElementTitle') }}</h3>
            <p class="mt-1 text-xs text-muted-foreground">{{ $t('learn.player.noElementBody') }}</p>
          </div>
        </div>

        <!-- ============================== -->
        <!-- NAVIGATION FOOTER              -->
        <!-- ============================== -->
        <div v-if="currentElement" class="flex-shrink-0 border-t border-border bg-card/60 px-3 pt-2 pb-[calc(0.5rem+var(--sab,env(safe-area-inset-bottom)))] md:px-6 md:py-3">
          <p v-if="claimError" class="mx-auto mb-2 max-w-4xl text-xs text-destructive">
            {{ $t('learn.player.finishError', { error: claimError }) }}
          </p>
          <div :class="['mx-auto flex items-center justify-between gap-2', isVideoElement ? 'max-w-7xl' : 'max-w-4xl']">
            <!-- Previous -->
            <AppButton variant="secondary" size="sm" :disabled="!hasPrevElement" @click="goToPrev">
              <svg class="mr-1.5 h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                <path stroke-linecap="round" stroke-linejoin="round" d="M15 19l-7-7 7-7" />
              </svg>
              {{ $t('learn.player.previous') }}
            </AppButton>

            <!-- Center action -->
            <AppButton
              v-if="!enrollment && course?.kind !== 'tutorial'"
              size="sm"
              :loading="enrolling"
              @click="enrollFromPlayer"
            >
              {{ $t('learn.player.enrollToTrack') }}
            </AppButton>
            <AppButton
              v-else-if="isContentElement && elementStatus(currentElement.id) !== 'completed'"
              class="bg-success text-success-foreground hover:opacity-90"
              size="sm"
              @click="markComplete()"
            >
              <svg class="mr-1.5 h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                <path stroke-linecap="round" stroke-linejoin="round" d="M5 13l4 4L19 7" />
              </svg>
              {{ $t('learn.player.markComplete') }}
            </AppButton>
            <span v-else class="text-xs text-muted-foreground">
              {{ activeFlatIndex + 1 }} / {{ flatElements.length }}
            </span>

            <!-- Next / Finish Course -->
            <AppButton
              v-if="isLastElement"
              size="sm"
              :loading="claiming"
              :disabled="!canFinish"
              :title="canFinish ? undefined : $t('learn.player.finishDisabledHint')"
              @click="finishCourse"
            >
              {{ $t('learn.player.finishCourse') }}
              <svg class="ml-1.5 h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                <path stroke-linecap="round" stroke-linejoin="round" d="M5 13l4 4L19 7" />
              </svg>
            </AppButton>
            <AppButton v-else size="sm" :disabled="!hasNextElement" @click="goToNext">
              {{ $t('common.actions.next') }}
              <svg class="ml-1.5 h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                <path stroke-linecap="round" stroke-linejoin="round" d="M9 5l7 7-7 7" />
              </svg>
            </AppButton>
          </div>
        </div>
      </div>
    </div>

    <!-- ============================== -->
    <!-- MOBILE: Chapter navigator sheet -->
    <!-- ============================== -->
    <Teleport to="body">
      <Transition
        enter-active-class="transition-opacity duration-150 ease-out"
        enter-from-class="opacity-0"
        leave-active-class="transition-opacity duration-150 ease-in"
        leave-to-class="opacity-0"
      >
        <div
          v-if="mobileNavOpen && !isTutorial"
          class="fixed inset-0 z-[120] bg-black/50 md:hidden"
          @click.self="mobileNavOpen = false"
        >
          <Transition
            enter-active-class="transition-transform duration-200 ease-out"
            enter-from-class="translate-y-full"
            leave-active-class="transition-transform duration-150 ease-in"
            leave-to-class="translate-y-full"
            appear
          >
            <div
              v-if="mobileNavOpen"
              class="absolute inset-x-0 bottom-0 max-h-[80vh] overflow-y-auto rounded-t-2xl border-t border-border bg-card pb-[calc(1rem+var(--sab,env(safe-area-inset-bottom)))]"
            >
              <!-- Grab handle + header -->
              <div class="sticky top-0 z-10 bg-card px-4 pt-2.5 pb-3">
                <div class="mx-auto mb-3 h-1 w-9 rounded-full bg-muted" />
                <div class="flex items-center justify-between">
                  <h2 class="text-sm font-semibold text-foreground">{{ $t('learn.player.chapters') }}</h2>
                  <button
                    class="rounded-md p-1 text-muted-foreground active:bg-muted"
                    :aria-label="$t('learn.player.closeChapterNav')"
                    @click="mobileNavOpen = false"
                  >
                    <svg class="h-5 w-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                      <path stroke-linecap="round" stroke-linejoin="round" d="M6 18L18 6M6 6l12 12" />
                    </svg>
                  </button>
                </div>
                <div class="mt-2 flex items-center gap-2">
                  <div class="h-1.5 flex-1 overflow-hidden rounded-full bg-muted/40">
                    <div
                      class="progress-fill h-full rounded-full"
                      :class="progressPercent === 100 ? 'bg-emerald-500' : 'bg-primary'"
                      :style="{ width: `${progressPercent}%` }"
                    />
                  </div>
                  <span class="text-[11px] text-muted-foreground">{{ completedElements }}/{{ totalElements }}</span>
                </div>
              </div>

              <!-- Chapter accordion -->
              <div class="px-2 pb-2">
                <div v-for="(ch, chIndex) in chapters" :key="ch.id" class="mb-1">
                  <button
                    class="flex w-full items-center gap-2 rounded-lg px-2 py-2.5 text-left active:bg-muted/50"
                    @click="toggleChapterExpanded(ch.id)"
                  >
                    <span
                      class="flex h-5 w-5 shrink-0 items-center justify-center rounded text-[10px] font-bold"
                      :class="(elements[ch.id] ?? []).length > 0 && (elements[ch.id] ?? []).every(el => elementStatus(el.id) === 'completed')
                        ? 'bg-emerald-100 text-emerald-700 dark:bg-emerald-900/30 dark:text-emerald-400'
                        : 'bg-muted/40 text-muted-foreground'"
                    >
                      <svg v-if="(elements[ch.id] ?? []).length > 0 && (elements[ch.id] ?? []).every(el => elementStatus(el.id) === 'completed')" class="h-3 w-3" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2.5">
                        <path stroke-linecap="round" stroke-linejoin="round" d="M5 13l4 4L19 7" />
                      </svg>
                      <template v-else>{{ chIndex + 1 }}</template>
                    </span>
                    <span class="min-w-0 flex-1 truncate text-xs font-medium uppercase tracking-wide text-muted-foreground">
                      {{ ch.title }}
                    </span>
                    <svg
                      class="h-4 w-4 shrink-0 text-muted-foreground transition-transform"
                      :class="expandedChapters.has(ch.id) ? 'rotate-180' : ''"
                      fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2"
                    >
                      <path stroke-linecap="round" stroke-linejoin="round" d="M19 9l-7 7-7-7" />
                    </svg>
                  </button>

                  <div v-show="expandedChapters.has(ch.id)" class="pl-1">
                    <button
                      v-for="el in elements[ch.id] ?? []"
                      :key="el.id"
                      class="flex w-full items-center gap-2.5 rounded-lg px-2 py-2.5 text-sm active:bg-muted/50"
                      :class="activeElement === el.id
                        ? 'bg-primary/10 text-primary font-medium ring-1 ring-primary/20'
                        : 'text-muted-foreground'"
                      @click="selectFromMobileNav(ch.id, el.id)"
                    >
                      <span
                        class="flex h-5 w-5 shrink-0 items-center justify-center rounded-full border"
                        :class="elementStatus(el.id) === 'completed'
                          ? 'border-emerald-500 bg-emerald-500 text-white'
                          : activeElement === el.id
                            ? 'border-primary bg-primary/10'
                            : elementStatus(el.id) === 'in_progress'
                              ? 'border-amber-400 bg-amber-400/10'
                              : 'border-border'"
                      >
                        <svg v-if="elementStatus(el.id) === 'completed'" class="h-2.5 w-2.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="3">
                          <path stroke-linecap="round" stroke-linejoin="round" d="M5 13l4 4L19 7" />
                        </svg>
                        <svg v-else class="h-2.5 w-2.5" :class="activeElement === el.id ? '' : 'opacity-50'" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                          <path stroke-linecap="round" stroke-linejoin="round" :d="elementTypeIcon(el.element_type)" />
                        </svg>
                      </span>
                      <span class="truncate text-left text-[13px]">{{ el.title }}</span>
                    </button>
                  </div>
                </div>
              </div>
            </div>
          </Transition>
        </div>
      </Transition>
    </Teleport>
  </div>
</template>

<style scoped>
.lesson-body {
  animation: lesson-body-in 0.22s ease;
}

.progress-fill {
  transition: width 560ms cubic-bezier(0.22, 1, 0.36, 1);
}

@keyframes lesson-body-in {
  from {
    opacity: 0;
    transform: translateY(6px);
  }
  to {
    opacity: 1;
    transform: translateY(0);
  }
}

@media (prefers-reduced-motion: reduce) {
  .lesson-body {
    animation: none;
  }
}
</style>
