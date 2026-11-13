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
  ProbeAnswerReceipt,
  ServedProbe,
  VerificationSummary,
} from '@/types'

export function useAssessment() {
  const { invoke } = useLocalApi()

  /** Published sponsor roles the learner may assess this skill for. Empty
   *  means the runner shows no role chooser. */
  function openRoles(skillId: string): Promise<AttemptRoleTarget[]> {
    return invoke<AttemptRoleTarget[]>('assessment_open_roles', { skillId })
  }

  /** `llmProbeConsent`: the learner agreed that, for a role whose
   *  follow-ups are generated, their chosen answers and the skill description
   *  may leave the device after submission. Recorded on the first start. */
  function startAttempt(
    skillId: string,
    integritySessionId: string,
    roleAssessmentId: string | null = null,
    llmProbeConsent = false,
  ): Promise<StartedAttempt> {
    return invoke<StartedAttempt>('assessment_start_attempt', {
      skillId,
      integritySessionId,
      roleAssessmentId,
      llmProbeConsent,
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

  /** Probes run after submission and before monitoring ends. Start serves
   *  the attempt's probes (idempotent); render starts a probe's time box;
   *  answer grades it host-side; finish records whatever was left pending
   *  as skipped. Skipped never counts against the learner. */
  function startProbes(attemptId: string): Promise<ServedProbe[]> {
    return invoke<ServedProbe[]>('assessment_probes_start', { attemptId })
  }

  function renderProbe(probeId: string): Promise<ServedProbe> {
    return invoke<ServedProbe>('assessment_probe_render', { probeId })
  }

  function answerProbe(
    probeId: string,
    response: Record<string, unknown>,
    firstInputMs: number | null,
  ): Promise<ProbeAnswerReceipt> {
    return invoke<ProbeAnswerReceipt>('assessment_probe_answer', { probeId, response, firstInputMs })
  }

  function finishProbes(attemptId: string): Promise<VerificationSummary> {
    return invoke<VerificationSummary>('assessment_probes_finish', { attemptId })
  }

  return {
    openRoles, startAttempt, saveDraft, submitAnswers, grade, planGoal,
    startProbes, renderProbe, answerProbe, finishProbes,
  }
}
