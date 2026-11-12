// Response-process collector: records when each item became available, the
// learner's response events on it, and cross-modal cues (a gaze-down, a
// second voice), and reduces them to per-visit aggregates. The input timeline
// never leaves the device; the backend judges the aggregates
// (`alexandria_studio::integrity::ResponseFeatures`).
//
// Works the same for a choice click, a typed answer and a plugin event: the
// caller says only which item an input belongs to and whether it changed an
// answer already given.
import type {
  ItemResponse,
  ResponseBaseline,
  ResponseBaselineItem,
  ResponseFeatures,
  ResponseInputKind,
} from '@/types'

/** No input anywhere for this long counts as idle. */
export const IDLE_MS = 20_000
/** Inputs this close after an idle stretch are one burst. */
export const BURST_MS = 2_000
/** Inputs a burst needs, per input kind: a click carries more than a key. */
const BURST_INPUTS: Record<ResponseInputKind, number> = { choice: 3, plugin: 3, text: 15 }
/** A response this soon after a cue is counted with it. */
export const CROSS_MODAL_MS = 3_000
/** First visits that make an in-attempt baseline. */
export const BASELINE_ITEMS = 3
/** First visits kept on the device as the learner's history. */
export const HISTORY_ITEMS = 12

export type ResponseCue = 'gaze_down' | 'second_voice'

export interface ResponseItemMeta {
  /** Characters to read: prompt plus options. */
  chars: number
  difficulty: number | null
  input: ResponseInputKind
}

interface Visit {
  itemId: string
  meta: ResponseItemMeta
  revisit: boolean
  startedAt: number
  firstInputAt: number
  lastInputAt: number
  revisions: number
  idleBursts: number
  changesAfterIdle: number
  /** The burst after the latest idle stretch, until it is counted or lapses. */
  burst: { since: number; inputs: number } | null
}

export interface ResponseTracker {
  /** The item is on screen and can be read. Repeats keep the first time. */
  present(itemId: string, meta: ResponseItemMeta): void
  /** A response event on the item. Unknown items are ignored. */
  input(itemId: string, event: { changesAnswer: boolean }): void
  /** A cross-modal cue: a gaze-down, or a second voice. */
  cue(kind: ResponseCue): void
  /** Ends the open visit (the attempt is ending). */
  settle(): void
  /** This window's response features, or null when there is nothing. Pure. */
  pending(): ResponseFeatures | null
  /** The window was submitted: start the next one. */
  clearWindow(): void
  /** Seeds the history kept from earlier sessions on this device. */
  setHistory(items: ResponseBaselineItem[]): void
  /** History to keep once this session ends. */
  historyAfterSession(): ResponseBaselineItem[]
  /** Forgets everything: a new session. */
  reset(): void
}

export function createResponseTracker(now: () => number = () => Date.now()): ResponseTracker {
  let presented = new Map<string, { meta: ResponseItemMeta; at: number }>()
  let visited = new Set<string>()
  let open: Visit | null = null
  let settled: ItemResponse[] = []
  let lastInputAt: number | null = null
  let sessionItems: ResponseBaselineItem[] = []
  let history: ResponseBaselineItem[] = []
  let cues: Record<ResponseCue, number[]> = { gaze_down: [], second_voice: [] }
  let cueResponses: Record<ResponseCue, number> = { gaze_down: 0, second_voice: 0 }

  const settle = () => {
    const v = open
    if (!v) return
    open = null
    settled.push({
      item_id: v.itemId,
      input: v.meta.input,
      chars: v.meta.chars,
      difficulty: v.meta.difficulty,
      revisit: v.revisit,
      first_input_ms: v.firstInputAt - v.startedAt,
      commit_ms: v.lastInputAt - v.startedAt,
      revisions: v.revisions,
      idle_bursts: v.idleBursts,
      changes_after_idle: v.changesAfterIdle,
    })
    if (!v.revisit) {
      sessionItems.push({
        chars: v.meta.chars,
        difficulty: v.meta.difficulty,
        first_input_ms: v.firstInputAt - v.startedAt,
      })
    }
  }

  const baseline = (): ResponseBaseline | null => {
    if (history.length >= BASELINE_ITEMS) return { source: 'history', items: history.slice(-HISTORY_ITEMS) }
    if (sessionItems.length >= BASELINE_ITEMS) return { source: 'attempt', items: sessionItems.slice(0, BASELINE_ITEMS) }
    return null
  }

  return {
    present(itemId, meta) {
      if (!presented.has(itemId)) presented.set(itemId, { meta, at: now() })
    },

    input(itemId, { changesAnswer }) {
      const item = presented.get(itemId)
      if (!item) return
      const t = now()
      const firstOfNewItem = open?.itemId !== itemId && !visited.has(itemId)
      // The wait before the first input on a new item is reading, measured
      // as latency; idle is a stretch with nothing new to read.
      const idle = !firstOfNewItem && lastInputAt !== null && t - lastInputAt >= IDLE_MS
      if (open?.itemId !== itemId) {
        settle()
        visited.add(itemId)
        // Items are read in turn: the next one starts once the learner has
        // finished with the last, not when the whole list rendered.
        const startedAt = firstOfNewItem ? Math.max(item.at, lastInputAt ?? item.at) : t
        open = {
          itemId, meta: item.meta, revisit: !firstOfNewItem, startedAt, firstInputAt: t, lastInputAt: t,
          revisions: 0, idleBursts: 0, changesAfterIdle: 0, burst: null,
        }
      }
      const v = open!
      if (changesAnswer) {
        v.revisions++
        if (idle) v.changesAfterIdle++
      }
      if (idle) v.burst = { since: t, inputs: 0 }
      if (v.burst) {
        if (t - v.burst.since <= BURST_MS) {
          v.burst.inputs++
          if (v.burst.inputs >= BURST_INPUTS[v.meta.input]) {
            v.idleBursts++
            v.burst = null
          }
        } else {
          v.burst = null
        }
      }
      v.lastInputAt = t
      lastInputAt = t
      for (const kind of ['gaze_down', 'second_voice'] as const) {
        if (cues[kind].some(at => t - at <= CROSS_MODAL_MS)) {
          cueResponses[kind]++
          cues[kind] = []
        }
      }
    },

    cue(kind) {
      const t = now()
      cues[kind] = [...cues[kind].filter(at => t - at <= CROSS_MODAL_MS), t]
    },

    settle,

    pending() {
      if (settled.length === 0 && cueResponses.gaze_down === 0 && cueResponses.second_voice === 0) return null
      return {
        items: settled.map(item => ({ ...item })),
        baseline: baseline(),
        gaze_down_then_response: cueResponses.gaze_down,
        voice_then_response: cueResponses.second_voice,
      }
    },

    clearWindow() {
      settled = []
      cueResponses = { gaze_down: 0, second_voice: 0 }
    },

    setHistory(items) {
      history = items.slice(-HISTORY_ITEMS)
    },

    historyAfterSession() {
      return [...history, ...sessionItems].slice(-HISTORY_ITEMS)
    },

    reset() {
      presented = new Map()
      visited = new Set()
      open = null
      settled = []
      lastInputAt = null
      sessionItems = []
      history = []
      cues = { gaze_down: [], second_voice: [] }
      cueResponses = { gaze_down: 0, second_voice: 0 }
    },
  }
}
