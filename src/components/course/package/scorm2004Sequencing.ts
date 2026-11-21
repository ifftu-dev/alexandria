// The package description's SCORM 2004 sequencing, mapped onto the vendored
// runtime's `sequencing.activityTree` setting. The two shapes differ in names
// and in a few places in meaning, and the runtime silently ignores a key it
// does not know, so every rename below matters:
//
// - a measure threshold is the condition's `parameters.threshold`;
// - `reorder_children` is the runtime's `randomizeChildren`;
// - a rollup rule's child activity set is its `consideration`;
// - rollup, constrained-choice and randomisation controls go into
//   `sequencingControls`, delivery controls into `deliveryControls`;
// - a map's read flags default to true in the runtime, so the IMS and ADL
//   maps of one objective and target are merged into one entry with every
//   flag stated.
//
// Sequencing collections are already merged into each activity by import and
// are never passed on: the runtime would merge their rules a second time.

import type {
  PackageActivity,
  PackageAdlMapInfo,
  PackageMapInfo,
  PackageObjective,
  PackageSequencing,
  PackageSequencingRule,
} from '@/types'

type Settings = Record<string, unknown>

export interface Scorm2004SequencingSettings {
  activityTree: Settings
}

/** The runtime's sequencing settings for a SCORM 2004 package's activity tree. */
export function sequencingSettings(tree: PackageActivity): Scorm2004SequencingSettings {
  return { activityTree: activitySettings(tree) }
}

function activitySettings(activity: PackageActivity): Settings {
  const out: Settings = { id: activity.id, title: activity.title, isVisible: activity.visible }
  const sequencing = activity.sequencing
  if (sequencing) Object.assign(out, sequencingFields(sequencing))
  const threshold = activity.completion_threshold
  if (threshold) {
    out.completionThreshold = {
      completedByMeasure: threshold.completed_by_measure,
      minProgressMeasure: threshold.min_progress_measure,
      progressWeight: threshold.progress_weight,
    }
  }
  if (activity.hide_lms_ui?.length) out.hideLmsUi = [...activity.hide_lms_ui]
  if (activity.shared_data?.length) {
    out.sharedDataMaps = activity.shared_data.map(map => ({
      targetID: map.target_id,
      readSharedData: map.read_shared_data,
      writeSharedData: map.write_shared_data,
    }))
  }
  if (activity.children?.length) out.children = activity.children.map(activitySettings)
  return out
}

function sequencingFields(sequencing: PackageSequencing): Settings {
  const out: Settings = {}
  const controls: Settings = {}
  const mode = sequencing.control_mode
  if (mode) {
    Object.assign(controls, {
      choice: mode.choice,
      choiceExit: mode.choice_exit,
      flow: mode.flow,
      forwardOnly: mode.forward_only,
      useCurrentAttemptObjectiveInfo: mode.use_current_attempt_objective_info,
      useCurrentAttemptProgressInfo: mode.use_current_attempt_progress_info,
    })
  }
  const rollup = sequencing.rollup_rules
  if (rollup) {
    Object.assign(controls, {
      rollupObjectiveSatisfied: rollup.rollup_objective_satisfied,
      rollupProgressCompletion: rollup.rollup_progress_completion,
      objectiveMeasureWeight: rollup.objective_measure_weight,
    })
    if (rollup.rules?.length) {
      out.rollupRules = {
        rules: rollup.rules.map(rule => ({
          action: rule.action,
          consideration: rule.child_activity_set,
          minimumCount: rule.minimum_count,
          minimumPercent: rule.minimum_percent,
          conditionCombination: rule.condition_combination,
          conditions: rule.conditions.map(condition => ({ condition: condition.condition, operator: condition.operator })),
        })),
      }
    }
  }
  const constrained = sequencing.constrained_choice_considerations
  if (constrained) {
    Object.assign(controls, { preventActivation: constrained.prevent_activation, constrainChoice: constrained.constrain_choice })
  }
  const randomization = sequencing.randomization_controls
  if (randomization) {
    Object.assign(controls, {
      selectionTiming: randomization.selection_timing,
      randomizeChildren: randomization.reorder_children,
      randomizationTiming: randomization.randomization_timing,
    })
    if (randomization.select_count != null) controls.selectCount = randomization.select_count
  }
  if (Object.keys(controls).length) out.sequencingControls = controls

  const delivery = sequencing.delivery_controls
  if (delivery) {
    out.deliveryControls = {
      tracked: delivery.tracked,
      completionSetByContent: delivery.completion_set_by_content,
      objectiveSetByContent: delivery.objective_set_by_content,
    }
  }
  const rules = sequencing.sequencing_rules
  if (rules) {
    out.sequencingRules = {
      preConditionRules: (rules.pre_condition_rules ?? []).map(ruleSettings),
      exitConditionRules: (rules.exit_condition_rules ?? []).map(ruleSettings),
      postConditionRules: (rules.post_condition_rules ?? []).map(ruleSettings),
    }
  }
  const considerations = sequencing.rollup_considerations
  if (considerations) {
    out.rollupConsiderations = {
      requiredForSatisfied: considerations.required_for_satisfied,
      requiredForNotSatisfied: considerations.required_for_not_satisfied,
      requiredForCompleted: considerations.required_for_completed,
      requiredForIncomplete: considerations.required_for_incomplete,
      measureSatisfactionIfActive: considerations.measure_satisfaction_if_active,
    }
  }
  const limits = sequencing.limit_conditions
  if (limits?.attempt_limit != null) out.attemptLimit = limits.attempt_limit
  if (limits?.attempt_absolute_duration_limit != null) out.attemptAbsoluteDurationLimit = limits.attempt_absolute_duration_limit

  const objectives = sequencing.objectives
  if (objectives) {
    const adl = new Map((sequencing.adl_objectives ?? []).map(objective => [objective.id, objective.map_info]))
    // A primary objective without an identifier has no slot in the runtime.
    if (objectives.primary.id) out.primaryObjective = objectiveSettings(objectives.primary, adl)
    const others = (objectives.others ?? []).filter(objective => objective.id)
    if (others.length) out.objectives = others.map(objective => objectiveSettings(objective, adl))
  }
  return out
}

function ruleSettings(rule: PackageSequencingRule): Settings {
  return {
    action: rule.action,
    conditionCombination: rule.condition_combination,
    conditions: rule.conditions.map((condition) => {
      const out: Settings = { condition: condition.condition, operator: condition.operator }
      if (condition.referenced_objective) out.referencedObjective = condition.referenced_objective
      if (condition.measure_threshold != null) out.parameters = { threshold: condition.measure_threshold }
      return out
    }),
  }
}

const NO_IMS_MAP = { readSatisfiedStatus: false, readNormalizedMeasure: false, writeSatisfiedStatus: false, writeNormalizedMeasure: false }
const NO_ADL_MAP = {
  readRawScore: false, readMinScore: false, readMaxScore: false, readCompletionStatus: false, readProgressMeasure: false,
  writeRawScore: false, writeMinScore: false, writeMaxScore: false, writeCompletionStatus: false, writeProgressMeasure: false,
}

function imsMap(map: PackageMapInfo) {
  return {
    readSatisfiedStatus: map.read_satisfied_status,
    readNormalizedMeasure: map.read_normalized_measure,
    writeSatisfiedStatus: map.write_satisfied_status,
    writeNormalizedMeasure: map.write_normalized_measure,
  }
}

function adlMap(map: PackageAdlMapInfo) {
  return {
    readRawScore: map.read_raw_score,
    readMinScore: map.read_min_score,
    readMaxScore: map.read_max_score,
    readCompletionStatus: map.read_completion_status,
    readProgressMeasure: map.read_progress_measure,
    writeRawScore: map.write_raw_score,
    writeMinScore: map.write_min_score,
    writeMaxScore: map.write_max_score,
    writeCompletionStatus: map.write_completion_status,
    writeProgressMeasure: map.write_progress_measure,
  }
}

function objectiveSettings(objective: PackageObjective, adl: Map<string, PackageAdlMapInfo[]>): Settings {
  const id = objective.id ?? ''
  const targets = new Map<string, Settings>()
  for (const map of objective.map_info ?? []) {
    targets.set(map.target_objective_id, { targetObjectiveID: map.target_objective_id, ...imsMap(map), ...NO_ADL_MAP })
  }
  for (const map of adl.get(id) ?? []) {
    const merged = targets.get(map.target_objective_id) ?? { targetObjectiveID: map.target_objective_id, ...NO_IMS_MAP }
    targets.set(map.target_objective_id, { ...merged, ...adlMap(map) })
  }
  return {
    objectiveID: id,
    satisfiedByMeasure: objective.satisfied_by_measure,
    minNormalizedMeasure: objective.min_normalized_measure,
    mapInfo: [...targets.values()],
  }
}
