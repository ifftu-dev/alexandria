// Dynamic assessment runner API. An attempt draws a randomized, difficulty-
// stratified question subset (host-side); answers are graded host-side (the
// key never reaches the client); passing issues an AssessmentCredential bound
// to the Sentinel integrity session, raising the skill's confidence.

import { useLocalApi } from './useLocalApi'
import type {
  AttemptRoleTarget,
  StartedAttempt,
  SubmittedAnswer,
  GradeResult,
  GoalAssessmentPlan,
} from '@/types'

export function useAssessment() {
  const { invoke } = useLocalApi()

  /** Published sponsor roles the learner may assess this skill for. Empty
   *  means the runner shows no role chooser. */
  function openRoles(skillId: string): Promise<AttemptRoleTarget[]> {
    return invoke<AttemptRoleTarget[]>('assessment_open_roles', { skillId })
  }

  function startAttempt(
    skillId: string,
    integritySessionId: string,
    roleAssessmentId: string | null = null,
  ): Promise<StartedAttempt> {
    return invoke<StartedAttempt>('assessment_start_attempt', {
      skillId,
      integritySessionId,
      roleAssessmentId,
    })
  }

  function submitAnswers(attemptId: string, answers: SubmittedAnswer[]): Promise<void> {
    return invoke('assessment_submit_answers', { attemptId, answers })
  }

  function grade(attemptId: string, answers: SubmittedAnswer[]): Promise<GradeResult> {
    return invoke<GradeResult>('assessment_grade', { attemptId, answers })
  }

  function saveDraft(attemptId: string, answers: SubmittedAnswer[]): Promise<void> {
    return invoke('assessment_save_draft', { attemptId, answers })
  }

  /** Order a goal's skills by prerequisite and annotate each with whether it
   *  can be assessed right now. Drives the goal-assessment sequence. */
  function planGoal(goalSkillIds: string[]): Promise<GoalAssessmentPlan> {
    return invoke<GoalAssessmentPlan>('assessment_plan_goal', { goalSkillIds })
  }

  return { openRoles, startAttempt, saveDraft, submitAnswers, grade, planGoal }
}
