// The candidate's side of the interview signal feed, in an interview held in
// Alexandria with the integrity check and voice checks allowed.
//
// The panel marks when it asks a question. This device waits for the
// question to be heard (the far end's playback goes quiet), presents it to
// the response tracker as a spoken item, and takes the candidate's first
// speech after it as the answer. Each answer goes to the panel with its
// latency, the candidate's own baseline and the cues just before it; a
// second voice goes to the panel as soon as it is heard. Timings and cues
// only: no audio, no words.
import type { InterviewAnswerSignal, InterviewSignal, VoiceOnsetEvent } from '@/types'
import type { ResponseTracker } from '@/utils/sentinel/responseProcess'

/** Playback level at which the far end counts as speaking. */
export const REMOTE_AUDIBLE = 0.05
/** Quiet after the far end spoke that ends a question. */
export const QUESTION_QUIET_MS = 400
/** A marker with no question spoken after it lapses. */
export const MARKER_LAPSE_MS = 30_000
/** Added lag over the candidate's baseline that the panel is shown as a concern. */
export const RELAY_CONCERN_MS = 3_000

export type AnswerConcern = 'relay_lag' | 'gaze_down' | 'second_voice'

export interface InterviewFeedDeps {
  responses: Pick<ResponseTracker, 'present' | 'input' | 'cue' | 'latest'>
  send: (signal: InterviewSignal) => void
  now: () => number
}

export interface InterviewFeed {
  /** The panel marked that it is asking a question. */
  question(questionId: string): void
  /** The far end's playback level on this device. */
  remoteLevel(level: number): void
  /** The voice detector heard a voice start. */
  voice(onset: VoiceOnsetEvent): void
}

/** What in an answer the panel may want to ask about. */
export function answerConcerns(answer: InterviewAnswerSignal): AnswerConcern[] {
  const concerns: AnswerConcern[] = []
  if (answer.baseline_ms !== null && answer.latency_ms - answer.baseline_ms >= RELAY_CONCERN_MS) concerns.push('relay_lag')
  if (answer.gaze_down_before) concerns.push('gaze_down')
  if (answer.second_voice_before) concerns.push('second_voice')
  return concerns
}

export function createInterviewFeed({ responses, send, now }: InterviewFeedDeps): InterviewFeed {
  /** A marked question not yet heard to its end. */
  let marked: { id: string; since: number; heardAt: number | null } | null = null
  /** A heard question waiting for its answer. */
  let asked: string | null = null
  /** The question being answered, for the candidate's later onsets. */
  let answering: string | null = null

  return {
    question(questionId) {
      marked = { id: questionId, since: now(), heardAt: null }
      asked = null
    },

    remoteLevel(level) {
      if (!marked) return
      const t = now()
      if (marked.heardAt === null && t - marked.since > MARKER_LAPSE_MS) {
        marked = null
      } else if (level >= REMOTE_AUDIBLE) {
        marked.heardAt = t
      } else if (marked.heardAt !== null && t - marked.heardAt >= QUESTION_QUIET_MS) {
        responses.present(marked.id, { chars: 0, difficulty: null, input: 'voice' }, marked.heardAt)
        asked = marked.id
        marked = null
      }
    },

    voice({ at_ms, speaker }) {
      if (speaker === 'other') {
        responses.cue('second_voice', at_ms)
        send('second_voice')
        return
      }
      // The candidate was talking before the question was over: no timing.
      if (marked) {
        marked = null
        return
      }
      if (asked) {
        const id = asked
        asked = null
        answering = id
        responses.input(id, { changesAnswer: false, at: at_ms })
        const latest = responses.latest()
        if (latest?.itemId !== id) return
        send({
          answer: {
            question_id: id,
            latency_ms: Math.round(latest.latencyMs),
            baseline_ms: latest.baselineMs === null ? null : Math.round(latest.baselineMs),
            gaze_down_before: latest.cues.includes('gaze_down'),
            second_voice_before: latest.cues.includes('second_voice'),
          },
        })
        return
      }
      if (answering) responses.input(answering, { changesAnswer: false, at: at_ms })
    },
  }
}
