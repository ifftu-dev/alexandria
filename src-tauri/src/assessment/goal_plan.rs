//! Goal-driven assessment planning.
//!
//! A learner picks a goal — a role, an exam, a job description — which
//! resolves to a set of target skills. This turns that set into an ordered,
//! annotated plan: which skills to assess, in prerequisite order, and which
//! can be assessed right now.
//!
//! # A sequence, not a single exam
//!
//! A goal assessment is a *dynamically assembled sequence* of ordinary
//! single-skill attempts, not one monolithic exam. Each skill is assessed
//! through the same path everything else uses — same grader, same attempt
//! policy, same per-skill credential. That is a deliberate reuse choice with
//! three consequences worth stating:
//!
//! * per-skill cooldowns apply without a new attribution rule (a single
//!   multi-skill exam would have to decide which skill a failure counts
//!   against);
//! * the learner earns a credential per skill as they go, rather than all or
//!   nothing at the end;
//! * a true single-sitting exam can be layered on later — this ordering and
//!   assessability logic is exactly what it would need first.
//!
//! # Prerequisite order
//!
//! Ordering comes from [`crate::commands::graph::compute_path`], which already
//! sorts by longest prerequisite chain and guards cycles. Assembling here
//! keeps that single source of truth rather than re-deriving an order.

use serde::{Deserialize, Serialize};

use crate::assessment::policy::PolicyDecision;
use crate::domain::bloom::BloomLevel;

/// One skill from the learning path, before assessability is layered on.
/// Mapped from `compute_path`'s step so this module need not depend on the
/// graph command's richer type.
#[derive(Debug, Clone)]
pub struct PlanStepInput {
    pub skill_id: String,
    pub name: String,
    pub bloom_level: BloomLevel,
    /// `"earned" | "available" | "locked"`, from the learning path.
    pub status: String,
    pub is_goal: bool,
}

/// What is known about assessing one skill right now.
#[derive(Debug, Clone)]
pub struct AssessInfo {
    /// A ratified bank exists for the skill.
    pub has_bank: bool,
    /// The attempt-policy decision, or `None` when there is no bank to
    /// evaluate against.
    pub decision: Option<PolicyDecision>,
}

/// One skill in the plan, with assessability resolved.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GoalAssessmentStep {
    pub skill_id: String,
    pub name: String,
    pub bloom_level: String,
    /// `"earned" | "available" | "locked"`.
    pub status: String,
    pub is_goal: bool,
    /// A ratified assessment exists for this skill.
    pub has_assessment: bool,
    /// The learner can start an attempt right now: unlocked, not yet earned,
    /// has an assessment, and not in cooldown.
    pub assessable_now: bool,
    /// When an attempt becomes available again, if currently cooling down.
    pub cooldown_until: Option<String>,
    /// Why this skill cannot be assessed right now, for display. `None` when
    /// `assessable_now` is true.
    pub blocked_reason: Option<String>,
}

/// An ordered goal assessment plan.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GoalAssessmentPlan {
    pub steps: Vec<GoalAssessmentStep>,
    /// How many skills can be assessed right now.
    pub assessable_count: usize,
    /// The first assessable skill in prerequisite order — the natural "start
    /// here". `None` when nothing is assessable yet.
    pub next_skill_id: Option<String>,
    /// Goal skills already earned, over goal skills total. A readiness
    /// fraction the UI can show without recomputing.
    pub goals_earned: usize,
    pub goals_total: usize,
}

/// Assemble the plan from ordered steps and per-skill assessability.
///
/// Pure: all database work (resolving banks, loading attempt history,
/// evaluating policy) happens before this, so the assembly and the choice of
/// "next" are unit-testable without a connection.
pub fn assemble_goal_plan(
    steps: &[PlanStepInput],
    info: &std::collections::HashMap<String, AssessInfo>,
) -> GoalAssessmentPlan {
    let mut out = Vec::with_capacity(steps.len());
    let mut assessable_count = 0;
    let mut next_skill_id = None;
    let mut goals_earned = 0;
    let mut goals_total = 0;

    for step in steps {
        if step.is_goal {
            goals_total += 1;
            if step.status == "earned" {
                goals_earned += 1;
            }
        }

        let assess = info.get(&step.skill_id);
        let has_assessment = assess.map(|a| a.has_bank).unwrap_or(false);

        let (assessable_now, cooldown_until, blocked_reason) =
            resolve_assessability(&step.status, has_assessment, assess);

        if assessable_now {
            assessable_count += 1;
            // First in prerequisite order wins — `steps` is already ordered.
            if next_skill_id.is_none() {
                next_skill_id = Some(step.skill_id.clone());
            }
        }

        out.push(GoalAssessmentStep {
            skill_id: step.skill_id.clone(),
            name: step.name.clone(),
            bloom_level: step.bloom_level.as_str().to_string(),
            status: step.status.clone(),
            is_goal: step.is_goal,
            has_assessment,
            assessable_now,
            cooldown_until,
            blocked_reason,
        });
    }

    GoalAssessmentPlan {
        steps: out,
        assessable_count,
        next_skill_id,
        goals_earned,
        goals_total,
    }
}

/// Decide whether one step is assessable now, and if not, why.
fn resolve_assessability(
    status: &str,
    has_assessment: bool,
    assess: Option<&AssessInfo>,
) -> (bool, Option<String>, Option<String>) {
    // Already proven — nothing to assess, and this is not a "blocked" state.
    if status == "earned" {
        return (false, None, None);
    }
    if status == "locked" {
        return (false, None, Some("prerequisites not yet met".to_string()));
    }
    if !has_assessment {
        return (false, None, Some("no assessment available yet".to_string()));
    }

    // Available, has an assessment — the attempt policy is the last word.
    match assess.and_then(|a| a.decision.as_ref()) {
        Some(PolicyDecision::Allow { .. }) => (true, None, None),
        Some(PolicyDecision::Cooldown { until, .. }) => {
            (false, Some(until.clone()), Some("in cooldown".to_string()))
        }
        Some(PolicyDecision::Exhausted { .. }) => {
            (false, None, Some("attempt limit reached".to_string()))
        }
        // No decision computed despite a bank existing — treat as assessable
        // and let the start path make the final call, rather than blocking on
        // missing information.
        None => (true, None, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn step(id: &str, status: &str, is_goal: bool) -> PlanStepInput {
        PlanStepInput {
            skill_id: id.to_string(),
            name: format!("Skill {id}"),
            bloom_level: BloomLevel::Apply,
            status: status.to_string(),
            is_goal,
        }
    }

    fn with_bank(decision: PolicyDecision) -> AssessInfo {
        AssessInfo {
            has_bank: true,
            decision: Some(decision),
        }
    }

    fn allow() -> PolicyDecision {
        PolicyDecision::Allow { ordinal: 1 }
    }

    #[test]
    fn an_available_skill_with_a_bank_is_assessable() {
        let steps = [step("a", "available", true)];
        let info = HashMap::from([("a".to_string(), with_bank(allow()))]);
        let plan = assemble_goal_plan(&steps, &info);

        assert_eq!(plan.assessable_count, 1);
        assert_eq!(plan.next_skill_id.as_deref(), Some("a"));
        assert!(plan.steps[0].assessable_now);
        assert!(plan.steps[0].blocked_reason.is_none());
    }

    #[test]
    fn a_locked_skill_is_never_assessable() {
        let steps = [step("a", "locked", true)];
        let info = HashMap::from([("a".to_string(), with_bank(allow()))]);
        let plan = assemble_goal_plan(&steps, &info);

        assert_eq!(plan.assessable_count, 0);
        assert!(!plan.steps[0].assessable_now);
        assert_eq!(
            plan.steps[0].blocked_reason.as_deref(),
            Some("prerequisites not yet met")
        );
    }

    #[test]
    fn an_earned_skill_is_not_flagged_as_blocked() {
        // Done is not the same as blocked; the UI should not nag about it.
        let steps = [step("a", "earned", true)];
        let info = HashMap::new();
        let plan = assemble_goal_plan(&steps, &info);

        assert!(!plan.steps[0].assessable_now);
        assert!(plan.steps[0].blocked_reason.is_none());
        assert_eq!(plan.goals_earned, 1);
        assert_eq!(plan.goals_total, 1);
    }

    #[test]
    fn an_available_skill_without_a_bank_says_so() {
        let steps = [step("a", "available", true)];
        let info = HashMap::from([(
            "a".to_string(),
            AssessInfo {
                has_bank: false,
                decision: None,
            },
        )]);
        let plan = assemble_goal_plan(&steps, &info);

        assert!(!plan.steps[0].assessable_now);
        assert!(!plan.steps[0].has_assessment);
        assert_eq!(
            plan.steps[0].blocked_reason.as_deref(),
            Some("no assessment available yet")
        );
    }

    #[test]
    fn a_cooling_down_skill_exposes_its_availability_time() {
        let steps = [step("a", "available", true)];
        let info = HashMap::from([(
            "a".to_string(),
            with_bank(PolicyDecision::Cooldown {
                until: "2026-07-24T00:00:00Z".to_string(),
                attempts_used: 1,
            }),
        )]);
        let plan = assemble_goal_plan(&steps, &info);

        assert!(!plan.steps[0].assessable_now);
        assert_eq!(
            plan.steps[0].cooldown_until.as_deref(),
            Some("2026-07-24T00:00:00Z")
        );
        assert_eq!(plan.steps[0].blocked_reason.as_deref(), Some("in cooldown"));
    }

    #[test]
    fn an_exhausted_skill_is_blocked_with_a_reason() {
        let steps = [step("a", "available", false)];
        let info = HashMap::from([(
            "a".to_string(),
            with_bank(PolicyDecision::Exhausted {
                attempts_used: 3,
                max: 3,
            }),
        )]);
        let plan = assemble_goal_plan(&steps, &info);

        assert!(!plan.steps[0].assessable_now);
        assert_eq!(
            plan.steps[0].blocked_reason.as_deref(),
            Some("attempt limit reached")
        );
    }

    #[test]
    fn next_follows_prerequisite_order_not_input_convenience() {
        // Steps arrive already ordered by compute_path; "next" is the first
        // assessable one, so a later-but-assessable skill must not jump ahead
        // of an earlier one.
        let steps = [
            step("basics", "available", false),
            step("advanced", "available", true),
        ];
        let info = HashMap::from([
            ("basics".to_string(), with_bank(allow())),
            ("advanced".to_string(), with_bank(allow())),
        ]);
        let plan = assemble_goal_plan(&steps, &info);

        assert_eq!(plan.next_skill_id.as_deref(), Some("basics"));
        assert_eq!(plan.assessable_count, 2);
    }

    #[test]
    fn next_skips_blocked_earlier_skills() {
        // The earliest skill is locked; "next" should be the first one a
        // learner can actually start.
        let steps = [
            step("locked_one", "locked", false),
            step("ready", "available", true),
        ];
        let info = HashMap::from([
            ("locked_one".to_string(), with_bank(allow())),
            ("ready".to_string(), with_bank(allow())),
        ]);
        let plan = assemble_goal_plan(&steps, &info);

        assert_eq!(plan.next_skill_id.as_deref(), Some("ready"));
        assert_eq!(plan.assessable_count, 1);
    }

    #[test]
    fn readiness_counts_only_goal_skills_not_prerequisites() {
        // A goal's readiness is about the goal skills; prerequisites pulled
        // in by the closure should not dilute the fraction.
        let steps = [
            step("prereq", "earned", false),
            step("goal_a", "earned", true),
            step("goal_b", "available", true),
        ];
        let info = HashMap::from([("goal_b".to_string(), with_bank(allow()))]);
        let plan = assemble_goal_plan(&steps, &info);

        assert_eq!(plan.goals_total, 2);
        assert_eq!(plan.goals_earned, 1);
    }

    #[test]
    fn an_empty_goal_yields_an_empty_plan() {
        let plan = assemble_goal_plan(&[], &HashMap::new());
        assert_eq!(plan.assessable_count, 0);
        assert!(plan.next_skill_id.is_none());
        assert_eq!(plan.goals_total, 0);
    }
}
