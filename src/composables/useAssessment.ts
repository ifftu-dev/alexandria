// Dynamic assessment runner API. An attempt draws a randomized, difficulty-
// stratified question subset (host-side); answers are graded host-side (the
// key never reaches the client); passing issues an AssessmentCredential bound
// to the Sentinel integrity session, raising the skill's confidence.

import { useLocalApi } from './useLocalApi'
import type { StartedAttempt, SubmittedAnswer, GradeResult } from '@/types'

export function useAssessment() {
  const { invoke } = useLocalApi()

  function startAttempt(skillId: string, integritySessionId: string | null): Promise<StartedAttempt> {
    return invoke<StartedAttempt>('assessment_start_attempt', {
      skillId,
      integritySessionId,
    })
  }

  function grade(attemptId: string, answers: SubmittedAnswer[]): Promise<GradeResult> {
    return invoke<GradeResult>('assessment_grade', { attemptId, answers })
  }

  return { startAttempt, grade }
}
