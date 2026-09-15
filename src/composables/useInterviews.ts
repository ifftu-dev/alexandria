import { readonly, ref } from 'vue'
import { useLocalApi } from '@/composables/useLocalApi'
import type {
  AppendInterviewTranscriptRequest,
  CreateInterviewRequest,
  InterviewBundle,
  InterviewConsentRequest,
  InterviewCriterionStatus,
  InterviewFollowup,
  InterviewFollowupStatus,
  InterviewNote,
  InterviewParticipant,
  InterviewSession,
  InterviewTranscriptSegment,
} from '@/types'

const interviews = ref<InterviewSession[]>([])
const activeInterview = ref<InterviewBundle | null>(null)
const loading = ref(false)
const error = ref<string | null>(null)

export function useInterviews() {
  const { invoke } = useLocalApi()

  async function run<T>(operation: () => Promise<T>): Promise<T> {
    loading.value = true
    error.value = null
    try {
      return await operation()
    } catch (cause) {
      error.value = cause instanceof Error ? cause.message : String(cause)
      throw cause
    } finally {
      loading.value = false
    }
  }

  async function listInterviews(): Promise<InterviewSession[]> {
    await invoke<number>('interview_purge_expired')
    const result = await run(() => invoke<InterviewSession[]>('interview_list'))
    interviews.value = result
    return result
  }

  async function getInterview(id: string): Promise<InterviewBundle | null> {
    const result = await run(() => invoke<InterviewBundle | null>('interview_get', { id }))
    activeInterview.value = result
    return result
  }

  async function createInterview(req: CreateInterviewRequest): Promise<InterviewBundle> {
    const result = await run(() => invoke<InterviewBundle>('interview_create', { req }))
    activeInterview.value = result
    await listInterviews()
    return result
  }

  async function setStatus(
    id: string,
    status: InterviewSession['status'],
    tutoringSessionId?: string,
    integritySessionId?: string,
  ): Promise<InterviewBundle> {
    const result = await run(() => invoke<InterviewBundle>('interview_set_status', {
      id,
      status,
      tutoringSessionId,
      integritySessionId,
    }))
    activeInterview.value = result
    return result
  }

  async function recordConsent(
    participantId: string,
    consent: InterviewConsentRequest,
  ): Promise<InterviewParticipant> {
    const participant = await run(() => invoke<InterviewParticipant>('interview_record_consent', {
      participantId,
      consent,
    }))
    if (activeInterview.value) {
      const index = activeInterview.value.participants.findIndex(item => item.id === participant.id)
      if (index >= 0) activeInterview.value.participants[index] = participant
    }
    return participant
  }

  async function appendTranscript(
    req: AppendInterviewTranscriptRequest,
  ): Promise<InterviewTranscriptSegment> {
    const segment = await invoke<InterviewTranscriptSegment>('interview_append_transcript', { req })
    if (activeInterview.value?.session.id === req.session_id) {
      activeInterview.value.transcript.push(segment)
    }
    return segment
  }

  async function recommendFollowups(
    sessionId: string,
    sourceSegmentId?: string,
  ): Promise<InterviewFollowup[]> {
    const result = await invoke<InterviewFollowup[]>('interview_recommend_followups', {
      sessionId,
      sourceSegmentId,
    })
    if (activeInterview.value?.session.id === sessionId) activeInterview.value.followups = result
    return result
  }

  async function setFollowupStatus(id: string, status: InterviewFollowupStatus): Promise<void> {
    await invoke<void>('interview_set_followup_status', { id, status })
    const followup = activeInterview.value?.followups.find(item => item.id === id)
    if (followup) followup.status = status
  }

  async function setCriterion(
    id: string,
    status: InterviewCriterionStatus,
    notes?: string,
  ): Promise<void> {
    await invoke<void>('interview_set_criterion', { id, status, notes })
    const criterion = activeInterview.value?.criteria.find(item => item.id === id)
    if (criterion) {
      criterion.status = status
      criterion.notes = notes ?? null
    }
  }

  async function saveNote(sessionId: string, text: string, id?: string): Promise<InterviewNote> {
    const note = await invoke<InterviewNote>('interview_save_note', { sessionId, text, id })
    if (activeInterview.value?.session.id === sessionId) {
      const index = activeInterview.value.notes.findIndex(item => item.id === note.id)
      if (index >= 0) activeInterview.value.notes[index] = note
      else activeInterview.value.notes.push(note)
    }
    return note
  }

  async function generateSummary(sessionId: string): Promise<string> {
    const summary = await run(() => invoke<string>('interview_generate_summary', { sessionId }))
    if (activeInterview.value?.session.id === sessionId) activeInterview.value.session.summary = summary
    return summary
  }

  async function saveReview(sessionId: string, summary: string, conclusion: string): Promise<void> {
    await run(() => invoke<void>('interview_save_review', { sessionId, summary, conclusion }))
    if (activeInterview.value?.session.id === sessionId) {
      activeInterview.value.session.summary = summary
      activeInterview.value.session.conclusion = conclusion
    }
  }

  async function deleteInterview(id: string): Promise<void> {
    await run(() => invoke<void>('interview_delete', { id }))
    interviews.value = interviews.value.filter(interview => interview.id !== id)
    if (activeInterview.value?.session.id === id) activeInterview.value = null
  }

  return {
    interviews: readonly(interviews),
    activeInterview: readonly(activeInterview),
    loading: readonly(loading),
    error: readonly(error),
    listInterviews,
    getInterview,
    createInterview,
    setStatus,
    recordConsent,
    appendTranscript,
    recommendFollowups,
    setFollowupStatus,
    setCriterion,
    saveNote,
    generateSummary,
    saveReview,
    deleteInterview,
  }
}
