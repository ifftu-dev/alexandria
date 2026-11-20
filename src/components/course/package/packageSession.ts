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

import type {
  OpenedPackageLaunch,
  PackageActivity,
  PackageAttemptProgress,
  PackageCommit,
  PackageLaunchInit,
  PackageStandard,
} from '@/types'

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
    saving = saving.then(async () => {
      try {
        const progress = await invoke<PackageAttemptProgress>('package_attempt_commit', { commit })
        events.saveError(null)
        events.progress(progress)
      } catch (error) {
        events.saveError(String(error))
      }
    })
  }

  function onMessage(event: MessageEvent) {
    const data: unknown = event.data
    if (!data || typeof data !== 'object') return
    const message = data as Record<string, unknown>
    for (const waiter of [...waiters]) {
      if (waiter.matches(event)) waiter.resolve()
    }
    if (message.source === FROM_WRAPPER && event.source === wrapper) {
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
      }
      return
    }
    // The bootstrap posts from any document inside the package frame.
    if (message.source === FROM_PACKAGE && within(event.source, wrapper)) {
      if (message.type === 'focus-mode-request') events.focusRequest()
      else if (message.type === 'blocked' && typeof message.what === 'string' && BLOCKED_KINDS.has(message.what)) {
        events.blocked(message.what as BlockedKind, typeof message.detail === 'string' ? message.detail.slice(0, 200) : null)
      }
    }
  }

  function post(message: Record<string, unknown>) {
    wrapper?.postMessage({ ...message, source: FROM_PLAYER }, '*')
  }

  /** Loads the storage clearing page in `target` and waits for its answer. */
  async function clearStorage(target: HTMLIFrameElement): Promise<boolean> {
    const window = target.contentWindow
    const cleared = waitFor(
      (event) => event.source === window && (event.data as { type?: unknown })?.type === 'storage-cleared',
      CLEAR_TIMEOUT_MS,
    )
    target.src = `${launch.base_url}_alexandria/clear.html`
    return cleared
  }

  /** Clears storage, then loads the wrapper and waits until it is ready. */
  async function start(): Promise<void> {
    window.addEventListener('message', onMessage)
    await clearStorage(frame)
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
        (event) => event.source === wrapper && (event.data as { type?: unknown, activityId?: unknown })?.type === 'ended'
          && (event.data as { activityId?: unknown }).activityId === ending,
        END_TIMEOUT_MS,
      )
      post({ type: 'end' })
      await ended
      lessonId = null
    }
    await saving
  }

  /** Starts `activity` in a fresh API session with its stored state. */
  async function openLesson(activity: PackageActivity): Promise<PackageLaunchInit> {
    await endLesson()
    const init = await invoke<PackageLaunchInit>('package_attempt_initialise', { attemptId, activityId: activity.id })
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
   * Ends the session. Works whether or not the frame is still in the page:
   * when the player unmounts, the wrapper finishes the lesson as its page
   * goes away and its last commit still arrives here.
   */
  async function close(): Promise<PackageAttemptProgress | null> {
    if (closed) return null
    closed = true
    try {
      await endLesson()
      const helper = frame.isConnected ? null : document.createElement('iframe')
      if (helper) {
        helper.hidden = true
        helper.setAttribute('sandbox', PACKAGE_FRAME_SANDBOX)
        helper.setAttribute('allow', '')
        document.body.appendChild(helper)
      }
      await clearStorage(helper ?? frame)
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

  return { start, openLesson, endLesson, close }
}

export type PackageSession = ReturnType<typeof createPackageSession>
