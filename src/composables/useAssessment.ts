// Dynamic assessment runner API. An attempt draws a randomized, difficulty-
// stratified question subset (host-side); answers are graded host-side (the
// key never reaches the client); passing issues an AssessmentCredential bound
// to the Sentinel integrity session, raising the skill's confidence.

import { useLocalApi } from './useLocalApi'
import type {
  StartedAttempt,
  SubmittedAnswer,
  GradeResult,
  GoalAssessmentPlan,
} from '@/types'

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

  /** Order a goal's skills by prerequisite and annotate each with whether it
   *  can be assessed right now. Drives the goal-assessment sequence. */
  function planGoal(goalSkillIds: string[]): Promise<GoalAssessmentPlan> {
    return invoke<GoalAssessmentPlan>('assessment_plan_goal', { goalSkillIds })
  }

  return { startAttempt, grade, planGoal }
}
