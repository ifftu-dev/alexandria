// The host half of the package player: everything between the app and the
// wrapper page it loads at the package origin (`<prefix>/_alexandria/
// wrapper.html`, see `src-tauri/src/package_serving/wrapper.js` for the
// message protocol). One session is one open launch of one attempt.
//
// - Storage at the package origin is cleared before the wrapper loads and
//   again before the launch closes.
// - Each lesson runs in its own API session in the wrapper, with that
//   lesson's stored state from `package_attempt_initialise`.
// - Commits arrive coalesced from the wrapper and are persisted one at a
//   time, in order: each is the whole state of a lesson, so a reordered older
//   commit would overwrite a newer one.
// - Closing ends the lesson (the wrapper finishes it), waits for its last
//   commit, clears storage, closes the launch and only then terminates the
//   attempt, so a completed package's extracted files can be dropped.
// - cmi5: a unit has no run-time API. `package_cmi5_launch` opens its session
//   (and on desktop its own window, when it asks for one) and returns its
//   launch URL; the wrapper only frames it, and the unit talks xAPI to the
//   session's endpoint itself. Ending the lesson ends the session.
// - SCORM 2004: the sequencing engine in the wrapper decides which lesson is
//   delivered (`startSequencing`, `navigate`); the host stores the engine's
//   state after each navigation, in the same queue as commits, and restores
//   it at the next start. Closing suspends the package first.

import type {
  Cmi5Launch,
  OpenedPackageLaunch,
  PackageActivity,
  PackageAttemptProgress,
  PackageCommit,
  PackageLaunchInit,
  PackageSequencingState,
  PackageStandard,
} from '@/types'
import { sequencingSettings } from './scorm2004Sequencing'

export type PackageInvoke = <T>(command: string, args?: Record<string, unknown>) => Promise<T>

/** What content asked for that the player does not provide. */
export type BlockedKind = 'popup' | 'dialog' | 'plugin' | 'remote-resource'

export interface PackageSessionEvents {
  /** The host stored a commit or ended the attempt. */
  progress: (progress: PackageAttemptProgress) => void
  /** Persisting a commit failed (`message`), or a later one succeeded (`null`). */
  saveError: (message: string | null) => void
  blocked: (kind: BlockedKind, detail: string | null) => void
  /** Content asked for full screen; the player shows focus mode instead. */
  focusRequest: () => void
  /** The wrapper refused a launch (unsupported standard, bad address). */
  refused: (reason: string) => void
  /** cmi5: the unit returned to its return page; its lesson is over. */
  unitExited?: () => void
  /** cmi5: the running unit opened in its own window (`launch`) or that ended (`null`). */
  unitWindow?: (launch: Cmi5Launch | null) => void
  /** SCORM 2004: the controls and table of contents the engine allows now. */
  navigation?: (state: SequencingNavigation) => void
  /** SCORM 2004: the engine delivered this activity and it is launching. */
  delivered?: (activityId: string) => void
  /** SCORM 2004: the engine delivered nothing; the learner chooses a lesson. */
  undelivered?: () => void
  /** SCORM 2004: the engine's session is over (suspended, exited, finished). */
  sessionEnded?: (reason: string) => void
}

/** One activity of the SCORM 2004 engine's tree, as plain data. */
export interface SequencingNode {
  id: string
  title: string
  visible: boolean
  /** The engine's completion status for the activity. */
  completion: string
  /** A choice request for it is valid now. */
  choosable: boolean
  children: SequencingNode[]
}

/** What the SCORM 2004 engine allows the player's controls to do now. */
export interface SequencingNavigation {
  current: string | null
  continue: boolean
  previous: boolean
  /** Controls the delivered activity asks the player not to draw (hideLMSUI). */
  hidden: string[]
  /** The engine's session is running: suspend and exit apply. */
  active: boolean
  tree: SequencingNode | null
}

/** Requests the player's SCORM 2004 controls issue. */
export type NavigationRequest = 'continue' | 'previous' | 'choice' | 'suspendAll' | 'exitAll' | 'resumeAll'

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null
}

function sequencingNode(value: unknown): SequencingNode | null {
  if (!isRecord(value) || typeof value.id !== 'string') return null
  const children = Array.isArray(value.children) ? value.children : []
  return {
    id: value.id,
    title: typeof value.title === 'string' ? value.title : '',
    visible: value.visible !== false,
    completion: typeof value.completion === 'string' ? value.completion : 'unknown',
    choosable: value.choosable === true,
    children: children.map(sequencingNode).filter((node): node is SequencingNode => node !== null),
  }
}

function sequencingNavigation(message: Record<string, unknown>): SequencingNavigation {
  return {
    current: typeof message.current === 'string' ? message.current : null,
    continue: message.continue === true,
    previous: message.previous === true,
    hidden: Array.isArray(message.hidden) ? message.hidden.filter((item): item is string => typeof item === 'string') : [],
    active: message.active === true,
    tree: sequencingNode(message.tree),
  }
}

export interface PackageSessionOptions {
  invoke: PackageInvoke
  frame: HTMLIFrameElement
  launch: OpenedPackageLaunch
  attemptId: string
  standard: PackageStandard
  events: PackageSessionEvents
  /** How often the wrapper may hand over state while a lesson runs. */
  commitIntervalMs?: number
}

const FROM_PLAYER = 'alexandria-player'
const FROM_WRAPPER = 'alexandria-wrapper'
const FROM_PACKAGE = 'alexandria-package'
const CLEAR_TIMEOUT_MS = 5000
const READY_TIMEOUT_MS = 10000
const END_TIMEOUT_MS = 3000
const BLOCKED_KINDS: ReadonlySet<string> = new Set(['popup', 'dialog', 'plugin', 'remote-resource'])

/** Frame sandbox: scripts and forms at the package's own origin, nothing else. */
export const PACKAGE_FRAME_SANDBOX = 'allow-scripts allow-same-origin allow-forms'

/** The address of a lesson under the launch prefix, path segments encoded. */
export function lessonUrl(baseUrl: string, activity: PackageActivity): string {
  const launch = activity.launch
  if (!launch) throw new Error(`activity ${activity.id} is not launchable`)
  const path = launch.path.split('/').map(encodeURIComponent).join('/')
  return `${baseUrl}${path}${launch.parameters ?? ''}`
}

/** True when `source` is `root` or a frame somewhere inside it. */
function within(source: MessageEventSource | null, root: Window | null): boolean {
  if (!source || !root) return false
  let current = source as Window
  for (let depth = 0; depth < 16; depth++) {
    if (current === root) return true
    const parent = current.parent
    if (!parent || parent === current) return false
    current = parent
  }
  return false
}

/** 128 random bits as hex, for answers that must belong to one request. */
function randomNonce(): string {
  const bytes = crypto.getRandomValues(new Uint8Array(16))
  return Array.from(bytes, byte => byte.toString(16).padStart(2, '0')).join('')
}

interface Waiter {
  matches: (event: MessageEvent) => boolean
  resolve: () => void
}

export function createPackageSession(options: PackageSessionOptions) {
  const { invoke, frame, launch, attemptId, standard, events } = options
  const waiters = new Set<Waiter>()
  let wrapper: Window | null = null
  let lessonId: string | null = null
  let saving: Promise<void> = Promise.resolve()
  let closed = false
  /** cmi5: the running unit's session. */
  let unitSession: Cmi5Launch | null = null
  /** cmi5: the unit opening in progress, if any; openings run one at a time. */
  let unitOpening: Promise<void> = Promise.resolve()
  /** Counts lesson requests; only the latest one launches. */
  let requests = 0
  /** SCORM 2004: the package's lessons by activity id, once sequencing started. */
  let delivering: Map<string, PackageActivity> | null = null
  const launchUrl = new URL(launch.base_url)
  const launchOrigin = launchUrl.origin
  /**
   * The origins a message from the launch can carry. A URL on an app scheme
   * (`package://localhost`) has an opaque origin to the URL parser, but
   * WebKit names its scheme-handler origins by scheme and host; the opaque
   * `null` covers engines that do not.
   */
  const wrapperOrigins: ReadonlySet<string> = launchOrigin === 'null'
    ? new Set([`${launchUrl.protocol}//${launchUrl.host}`, 'null'])
    : new Set([launchOrigin])
  /** Where messages for the wrapper may go; `*` only when the origin is opaque. */
  const postTarget = launchOrigin === 'null' ? '*' : launchOrigin

  /**
   * The message comes from the wrapper page. WebKit delivers what a frame
   * posts while it is being removed (the player unmounting) with no source
   * window, so once the frame has left the page a sourceless message from
   * the launch's origin is taken as the wrapper's last words.
   */
  function fromWrapper(event: MessageEvent): boolean {
    if (wrapper !== null && event.source === wrapper) return true
    return wrapper !== null && event.source === null && !frame.isConnected && wrapperOrigins.has(event.origin)
  }

  function waitFor(matches: (event: MessageEvent) => boolean, timeoutMs: number): Promise<boolean> {
    return new Promise((resolve) => {
      const waiter: Waiter = {
        matches,
        resolve: () => {
          clearTimeout(timer)
          waiters.delete(waiter)
          resolve(true)
        },
      }
      const timer = setTimeout(() => {
        waiters.delete(waiter)
        resolve(false)
      }, timeoutMs)
      waiters.add(waiter)
    })
  }

  function persist(commit: PackageCommit) {
    store(() => invoke<PackageAttemptProgress>('package_attempt_commit', { commit }))
  }

  /** Queues one write behind the others: each is whole state, so order matters. */
  function store(write: () => Promise<PackageAttemptProgress>) {
    saving = saving.then(async () => {
      try {
        const progress = await write()
        events.saveError(null)
        events.progress(progress)
      } catch (error) {
        events.saveError(String(error))
      }
    })
  }

  /**
   * SCORM 2004: the engine delivered `activityId`. Its stored state is read
   * once every earlier write has landed, then the wrapper opens it.
   */
  async function deliver(activityId: string, resumed: boolean) {
    const activity = delivering?.get(activityId)
    if (!activity?.launch) {
      events.refused(`not-a-lesson ${activityId}`)
      return
    }
    await saving
    if (closed) return
    const init = await invoke<PackageLaunchInit>('package_attempt_initialise', { attemptId, activityId, resumed })
    lessonId = activityId
    post({
      type: 'launch',
      standard,
      activityId,
      title: activity.title,
      url: lessonUrl(launch.base_url, activity),
      runtime: init.runtime,
    })
    events.delivered?.(activityId)
    if (init.progress) events.progress(init.progress)
  }

  function onSequencingMessage(message: Record<string, unknown>) {
    switch (message.type) {
      case 'sequencing-state': {
        const root = isRecord(message.root) ? message.root : {}
        if (typeof message.snapshot !== 'string') return
        const sequencing: PackageSequencingState = {
          attempt_id: attemptId,
          snapshot: message.snapshot,
          root: { completion_status: typeof root.completion_status === 'string' ? root.completion_status : 'unknown' },
        }
        store(() => invoke<PackageAttemptProgress>('package_attempt_sequencing_save', { sequencing }))
        return
      }
      case 'navigation':
        events.navigation?.(sequencingNavigation(message))
        return
      case 'delivered':
        if (typeof message.activityId === 'string') {
          deliver(message.activityId, message.resumed === true).catch(error => events.refused(String(error)))
        }
        return
      case 'undelivered':
        events.undelivered?.()
        return
      case 'session-ended':
        events.sessionEnded?.(String(message.reason ?? ''))
    }
  }

  function onMessage(event: MessageEvent) {
    const data: unknown = event.data
    if (!data || typeof data !== 'object') return
    const message = data as Record<string, unknown>
    for (const waiter of [...waiters]) {
      if (waiter.matches(event)) waiter.resolve()
    }
    if (message.source === FROM_WRAPPER && fromWrapper(event)) {
      if (message.type === 'commit') {
        // Only the running lesson's state is taken; the host fills in the
        // attempt itself.
        const runtime = message.runtime as PackageCommit['runtime'] | undefined
        const activityId = lessonId
        if (activityId === null || message.activityId !== activityId || !runtime || typeof runtime !== 'object') return
        persist({ attempt_id: attemptId, activity_id: activityId, session_ended: message.sessionEnded === true, runtime })
      } else if (message.type === 'ended' && message.activityId === lessonId) {
        lessonId = null
      } else if (message.type === 'refused') {
        events.refused(String(message.reason ?? ''))
      } else if (delivering) {
        onSequencingMessage(message)
      }
      return
    }
    // The bootstrap posts from any document inside the package frame.
    if (message.source === FROM_PACKAGE && within(event.source, wrapper)) {
      if (message.type === 'focus-mode-request') events.focusRequest()
      else if (message.type === 'au-exited' && unitSession) events.unitExited?.()
      else if (message.type === 'blocked' && typeof message.what === 'string' && BLOCKED_KINDS.has(message.what)) {
        events.blocked(message.what as BlockedKind, typeof message.detail === 'string' ? message.detail.slice(0, 200) : null)
      }
    }
  }

  function post(message: Record<string, unknown>) {
    wrapper?.postMessage({ ...message, source: FROM_PLAYER }, postTarget)
  }

  /**
   * Loads the storage clearing page in `target` and waits for its answer:
   * true only when the page says everything was cleared. Every launch shares
   * the package origin, so this is the only thing between one launch's
   * storage and the next. The frame keeps its window across navigations, so
   * the document being replaced could post an answer too; the page echoes a
   * nonce it reads from its own address, which that document never sees.
   */
  async function clearStorage(target: HTMLIFrameElement): Promise<boolean> {
    const window = target.contentWindow
    const nonce = randomNonce()
    let ok = false
    const answered = waitFor((event) => {
      const data: unknown = event.data
      if (event.source !== window || !isRecord(data) || data.source !== FROM_PACKAGE
        || data.type !== 'storage-cleared' || data.nonce !== nonce) return false
      ok = data.ok === true
      return true
    }, CLEAR_TIMEOUT_MS)
    target.src = `${launch.base_url}_alexandria/clear.html#${nonce}`
    return await answered && ok
  }

  /** Clears storage, then loads the wrapper and waits until it is ready. */
  async function start(): Promise<void> {
    window.addEventListener('message', onMessage)
    if (!await clearStorage(frame)) throw new Error('the package storage could not be cleared')
    wrapper = frame.contentWindow
    const ready = waitFor(
      (event) => event.source === wrapper && (event.data as { type?: unknown, source?: unknown })?.source === FROM_WRAPPER
        && (event.data as { type?: unknown }).type === 'ready',
      READY_TIMEOUT_MS,
    )
    frame.src = `${launch.base_url}_alexandria/wrapper.html`
    if (!await ready) throw new Error('the package player did not start')
  }

  /** Ends the running lesson and waits for its last commit to be stored. */
  async function endLesson(): Promise<void> {
    if (lessonId !== null && wrapper) {
      const ending = lessonId
      const ended = waitFor(
        (event) => fromWrapper(event) && (event.data as { type?: unknown, activityId?: unknown })?.type === 'ended'
          && (event.data as { activityId?: unknown }).activityId === ending,
        END_TIMEOUT_MS,
      )
      post({ type: 'end' })
      await ended
      lessonId = null
    }
    await saving
    if (unitSession) await endUnit()
  }

  /** cmi5: ends the running unit's session and reports the attempt's progress. */
  async function endUnit(): Promise<void> {
    const ending = unitSession
    if (!ending) return
    unitSession = null
    if (ending.own_window) events.unitWindow?.(null)
    try {
      events.progress(await invoke<PackageAttemptProgress>('package_cmi5_end', { sessionId: ending.session_id }))
    } catch (error) {
      events.saveError(String(error))
    }
  }

  /** cmi5: the host ended a session without being asked (its own window closed). */
  function unitEnded(sessionId: string, progress: PackageAttemptProgress | null) {
    if (unitSession?.session_id !== sessionId) return
    if (unitSession.own_window) events.unitWindow?.(null)
    unitSession = null
    lessonId = null
    if (progress) events.progress(progress)
  }

  /** cmi5: brings the running unit's own window to the front. */
  async function focusUnitWindow(): Promise<boolean> {
    if (!unitSession?.own_window) return false
    return invoke<boolean>('package_cmi5_focus', { sessionId: unitSession.session_id })
  }

  /**
   * cmi5: opens `activity`'s session and frames its launch URL (or its own
   * window opened). Openings run one after another, so a unit's session is
   * known before the next one ends it; only the latest request launches, and
   * a session a newer request overtook is ended at once. `null` when this
   * request was overtaken.
   */
  async function openUnit(activity: PackageActivity, request: number): Promise<Cmi5Launch | null> {
    const previous = unitOpening
    let finished = () => {}
    unitOpening = new Promise<void>((resolve) => { finished = resolve })
    try {
      await previous
      if (request !== requests || closed) return null
      await endLesson()
      if (request !== requests || closed) return null
      const unit = await invoke<Cmi5Launch>('package_cmi5_launch', {
        attemptId,
        activityId: activity.id,
        prefix: launch.prefix,
      })
      unitSession = unit
      if (request !== requests || closed) {
        await endUnit()
        return null
      }
      if (unit.own_window) {
        events.unitWindow?.(unit)
        return unit
      }
      lessonId = activity.id
      post({ type: 'launch', standard, activityId: activity.id, title: activity.title, url: unit.url })
      return unit
    } finally {
      finished()
    }
  }

  /**
   * Starts `activity` in a fresh API session with its stored state. Only the
   * latest request launches: `null` when the learner picked another lesson
   * while this one was starting.
   */
  async function openLesson(activity: PackageActivity): Promise<PackageLaunchInit | Cmi5Launch | null> {
    const request = ++requests
    if (standard === 'cmi5') return openUnit(activity, request)
    await endLesson()
    const init = await invoke<PackageLaunchInit>('package_attempt_initialise', { attemptId, activityId: activity.id })
    if (request !== requests) return null
    lessonId = activity.id
    post({
      type: 'launch',
      standard,
      activityId: activity.id,
      title: activity.title,
      url: lessonUrl(launch.base_url, activity),
      runtime: init.runtime,
      commitIntervalMs: options.commitIntervalMs,
    })
    if (init.progress) events.progress(init.progress)
    return init
  }

  /**
   * SCORM 2004: hands the wrapper the package's sequencing and the engine
   * state stored for the attempt; the engine then resumes or starts and
   * says what it delivered.
   */
  async function startSequencing(tree: PackageActivity): Promise<void> {
    const lessons = new Map<string, PackageActivity>()
    const walk = (activity: PackageActivity) => {
      if (activity.launch) lessons.set(activity.id, activity)
      for (const child of activity.children ?? []) walk(child)
    }
    walk(tree)
    delivering = lessons
    const snapshot = await invoke<string | null>('package_attempt_sequencing', { attemptId })
    post({
      type: 'sequence',
      standard,
      settings: sequencingSettings(tree),
      snapshot,
      commitIntervalMs: options.commitIntervalMs,
    })
  }

  /** SCORM 2004: a request from the player's controls, decided by the engine. */
  function navigate(request: NavigationRequest, target?: string) {
    post({ type: 'navigate', request, target: target ?? null })
  }

  /** SCORM 2004: suspends the package in the wrapper and waits for its last state. */
  async function suspendForClose(): Promise<void> {
    if (wrapper) {
      const done = waitFor(
        (event) => fromWrapper(event) && (event.data as { type?: unknown, source?: unknown })?.source === FROM_WRAPPER
          && (event.data as { type?: unknown }).type === 'closed',
        END_TIMEOUT_MS,
      )
      post({ type: 'end' })
      await done
      lessonId = null
    }
    await saving
  }

  /**
   * Ends the session. Works whether or not the frame is still in the page:
   * when the player unmounts, the wrapper finishes the lesson as its page
   * goes away and its last commit still arrives here.
   */
  async function close(): Promise<PackageAttemptProgress | null> {
    if (closed) return null
    closed = true
    try {
      await unitOpening
      if (delivering) await suspendForClose()
      else await endLesson()
      const helper = frame.isConnected ? null : document.createElement('iframe')
      if (helper) {
        helper.hidden = true
        helper.setAttribute('sandbox', PACKAGE_FRAME_SANDBOX)
        helper.setAttribute('allow', '')
        document.body.appendChild(helper)
      }
      // A launch that cannot be cleared on the way out still closes; the
      // next launch clears again before anything runs and refuses to start
      // when that fails.
      if (!await clearStorage(helper ?? frame)) console.warn('package storage could not be cleared after the launch')
      helper?.remove()
      await invoke<boolean>('package_launch_close', { prefix: launch.prefix })
      const ended = await invoke<PackageAttemptProgress>('package_attempt_terminate', { attemptId })
      events.progress(ended)
      return ended
    } finally {
      window.removeEventListener('message', onMessage)
      waiters.clear()
    }
  }

  return { start, openLesson, endLesson, close, startSequencing, navigate, unitEnded, focusUnitWindow }
}

export type PackageSession = ReturnType<typeof createPackageSession>
