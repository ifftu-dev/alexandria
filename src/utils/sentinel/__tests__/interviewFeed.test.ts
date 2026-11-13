// Seam 5 for the interview signal feed: on the candidate's device, the
// panel's question markers, the far end's playback level and the voice
// detector's onsets become the signals the panel sees live. Nothing here is
// about scores; the feed carries timings and cues only.
import { describe, expect, it } from 'vitest'
import type { InterviewSignal } from '@/types'
import { answerConcerns, createInterviewFeed } from '@/utils/sentinel/interviewFeed'
import { createResponseTracker } from '@/utils/sentinel/responseProcess'

function harness() {
  let clock = 1_000_000
  const sent: InterviewSignal[] = []
  const responses = createResponseTracker(() => clock)
  const feed = createInterviewFeed({ responses, send: s => sent.push(s), now: () => clock })
  const at = (ms: number) => { clock = ms }
  /**
   * The interviewer speaks for `ms`, metered every 50 ms as the call does,
   * then falls quiet. Returns when the question was last heard.
   */
  const remoteSpeech = (ms: number) => {
    const end = clock + ms
    let lastHeard = clock
    for (; clock < end; clock += 50) {
      feed.remoteLevel(0.3)
      lastHeard = clock
    }
    for (let quiet = 0; quiet <= 600; quiet += 50, clock += 50) feed.remoteLevel(0.01)
    return lastHeard
  }
  return { feed, sent, responses, at, remoteSpeech, now: () => clock }
}

describe('interview signal feed', () => {
  it('times each answer from the end of the question and sends it with the candidate\'s own baseline', () => {
    const h = harness()
    const latencies = [600, 800, 700, 4_800]
    latencies.forEach((latency, i) => {
      h.feed.question(`q${i + 1}`)
      const questionEnded = h.remoteSpeech(3_000)
      h.feed.voice({ at_ms: questionEnded + latency, speaker: 'primary' })
      h.at(questionEnded + latency + 5_000)
    })
    expect(h.sent).toEqual([
      { answer: { question_id: 'q1', latency_ms: 600, baseline_ms: null, gaze_down_before: false, second_voice_before: false } },
      { answer: { question_id: 'q2', latency_ms: 800, baseline_ms: null, gaze_down_before: false, second_voice_before: false } },
      { answer: { question_id: 'q3', latency_ms: 700, baseline_ms: null, gaze_down_before: false, second_voice_before: false } },
      { answer: { question_id: 'q4', latency_ms: 4_800, baseline_ms: 700, gaze_down_before: false, second_voice_before: false } },
    ])
    expect(h.responses.pending()?.items.map(i => [i.item_id, i.input, i.first_input_ms])).toEqual([
      ['q1', 'voice', 600], ['q2', 'voice', 800], ['q3', 'voice', 700],
    ])
  })

  it('sends a second voice at once, and marks the answer it came before', () => {
    const h = harness()
    h.feed.question('q1')
    const ended = h.remoteSpeech(2_000)
    h.feed.voice({ at_ms: ended + 300, speaker: 'other' })
    h.feed.voice({ at_ms: ended + 2_100, speaker: 'primary' })
    expect(h.sent).toEqual([
      'second_voice',
      { answer: { question_id: 'q1', latency_ms: 2_100, baseline_ms: null, gaze_down_before: false, second_voice_before: true } },
    ])
  })

  it('marks an answer that came just after a gaze down', () => {
    const h = harness()
    h.feed.question('q1')
    const ended = h.remoteSpeech(2_000)
    h.responses.cue('gaze_down', ended + 1_000)
    h.feed.voice({ at_ms: ended + 1_500, speaker: 'primary' })
    expect(h.sent).toEqual([
      { answer: { question_id: 'q1', latency_ms: 1_500, baseline_ms: null, gaze_down_before: true, second_voice_before: false } },
    ])
  })

  it('times nothing when the candidate spoke before the question was over or no question followed the marker', () => {
    const h = harness()
    // The panel marked a question, but the candidate was already talking.
    h.feed.question('q1')
    h.feed.voice({ at_ms: h.now(), speaker: 'primary' })
    h.remoteSpeech(2_000)
    h.feed.voice({ at_ms: h.now() + 500, speaker: 'primary' })
    // A marker nobody spoke after lapses.
    h.feed.question('q2')
    h.at(h.now() + 60_000)
    h.remoteSpeech(2_000)
    h.feed.voice({ at_ms: h.now() + 500, speaker: 'primary' })
    expect(h.sent).toEqual([])
  })

  it('names the concerns in an answer for the panel', () => {
    const answer = { question_id: 'q', latency_ms: 4_500, baseline_ms: 800, gaze_down_before: true, second_voice_before: false }
    expect(answerConcerns(answer)).toEqual(['relay_lag', 'gaze_down'])
    expect(answerConcerns({ ...answer, latency_ms: 2_000 })).toEqual(['gaze_down'])
    expect(answerConcerns({ ...answer, baseline_ms: null, gaze_down_before: false, second_voice_before: true })).toEqual(['second_voice'])
  })
})
