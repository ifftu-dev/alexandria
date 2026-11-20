//! SCORM 2004 sequencing as an imported package describes it.
//!
//! These types carry what a manifest's `imsss:sequencing` element, and the
//! ADL extensions to it, say about one activity. They are what the run-time's
//! sequencing engine is configured from; this crate evaluates none of it.
//!
//! The shape follows the manifest: one struct per manifest element, one field
//! per attribute, field names in snake case, and enumerated values spelled
//! exactly as the manifest spells them (`"objectiveMeasureGreaterThan"`,
//! `"exitAll"`). An element the manifest leaves out is `None` (or an empty
//! list), and the engine applies the standard's default for it. An element
//! that is present has every attribute filled in, the manifest's value or
//! the standard's default, so a reader never has to know a default.
//!
//! A sequencing collection (`IDRef`) is already applied: an activity's
//! [`Sequencing`] is what it gets once the collection's elements are taken
//! for those the item does not define itself.

use serde::{Deserialize, Serialize};

/// A manifest vocabulary: an enumeration read from an attribute or text.
pub(crate) trait Vocabulary: Sized + Copy + 'static {
    /// Every value as the manifest spells it, space-separated.
    const VALUES: &'static str;
    /// Reads a manifest value. Case is ignored, because hand-edited
    /// manifests get it wrong and the meaning is not in doubt.
    fn parse(text: &str) -> Option<Self>;
}

/// Declares an enumeration whose values are a manifest vocabulary.
macro_rules! vocabulary {
    ($(#[$meta:meta])* $name:ident { $($variant:ident = $token:literal,)+ }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        pub enum $name {
            $(#[serde(rename = $token)] $variant,)+
        }

        impl $name {
            /// The value as the manifest spells it.
            pub fn as_str(self) -> &'static str {
                match self {
                    $($name::$variant => $token,)+
                }
            }
        }

        impl Vocabulary for $name {
            const VALUES: &'static str = concat!($($token, " "),+);

            fn parse(text: &str) -> Option<Self> {
                let text = text.trim();
                [$($name::$variant,)+]
                    .into_iter()
                    .find(|value| value.as_str().eq_ignore_ascii_case(text))
            }
        }
    };
}

/// One activity's sequencing definition.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Sequencing {
    /// The sequencing collection the item referred to (`IDRef`), already
    /// applied to the fields below. For the record only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collection: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub control_mode: Option<ControlMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sequencing_rules: Option<SequencingRules>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit_conditions: Option<LimitConditions>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rollup_rules: Option<RollupRules>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub objectives: Option<Objectives>,
    /// `adlseq:objectives`: the ADL extension of objective maps to scores,
    /// completion and progress (4th Edition).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub adl_objectives: Vec<AdlObjective>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub randomization_controls: Option<RandomizationControls>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivery_controls: Option<DeliveryControls>,
    /// `adlseq:constrainedChoiceConsiderations`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub constrained_choice_considerations: Option<ConstrainedChoiceConsiderations>,
    /// `adlseq:rollupConsiderations`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rollup_considerations: Option<RollupConsiderations>,
}

/// `imsss:controlMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ControlMode {
    /// Default true.
    pub choice: bool,
    /// Default true.
    pub choice_exit: bool,
    /// Default false.
    pub flow: bool,
    /// Default false.
    pub forward_only: bool,
    /// Default true.
    pub use_current_attempt_objective_info: bool,
    /// Default true.
    pub use_current_attempt_progress_info: bool,
}

/// `imsss:sequencingRules`, the three kinds in manifest order.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct SequencingRules {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pre_condition_rules: Vec<SequencingRule>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exit_condition_rules: Vec<SequencingRule>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub post_condition_rules: Vec<SequencingRule>,
}

/// One pre-condition, exit-condition or post-condition rule.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct SequencingRule {
    /// Default `all`.
    pub condition_combination: ConditionCombination,
    /// At least one.
    pub conditions: Vec<RuleCondition>,
    /// Always one the rule's kind allows: `skip`, `disabled`,
    /// `hiddenFromChoice`, `stopForwardTraversal` before; `exit` on exit;
    /// `exitParent`, `exitAll`, `retry`, `retryAll`, `continue`, `previous`
    /// after.
    pub action: RuleAction,
}

/// `imsss:ruleCondition`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct RuleCondition {
    pub condition: RuleConditionKind,
    /// Default `noOp`.
    pub operator: ConditionOperator,
    /// An objective of this activity, by id. Absent means the primary
    /// objective.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub referenced_objective: Option<String>,
    /// From -1 to 1. Always set for the two measure comparisons (default 0),
    /// set elsewhere only when the manifest states it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub measure_threshold: Option<f64>,
}

/// `imsss:limitConditions`. SCORM 2004 uses only these two of the IMS
/// limits; the others are not read.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct LimitConditions {
    /// Attempts allowed. IMS gives 0 as the default and reads it as no limit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempt_limit: Option<u32>,
    /// An XML Schema duration (`PT30M`). Also handed to the lesson as
    /// `cmi.max_time_allowed`, see [`super::Launch::max_time_allowed`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempt_absolute_duration_limit: Option<String>,
}

/// `imsss:rollupRules`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct RollupRules {
    /// Default true.
    pub rollup_objective_satisfied: bool,
    /// Default true.
    pub rollup_progress_completion: bool,
    /// From 0 to 1, default 1.
    pub objective_measure_weight: f64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<RollupRule>,
}

/// `imsss:rollupRule`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct RollupRule {
    /// Default `all`.
    pub child_activity_set: ChildActivitySet,
    /// Default 0. Read with `atLeastCount`.
    pub minimum_count: u32,
    /// From 0 to 1, default 0. Read with `atLeastPercent`.
    pub minimum_percent: f64,
    /// Default `any`: note that rollup's default differs from the
    /// sequencing rules' `all`.
    pub condition_combination: ConditionCombination,
    /// At least one.
    pub conditions: Vec<RollupCondition>,
    pub action: RollupAction,
}

/// `imsss:rollupCondition`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct RollupCondition {
    pub condition: RollupConditionKind,
    /// Default `noOp`.
    pub operator: ConditionOperator,
}

/// `imsss:objectives`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Objectives {
    /// `imsss:primaryObjective`, which the element must have.
    pub primary: Objective,
    /// Every `imsss:objective`, in manifest order. Each has an id, and no id
    /// is used twice within the activity.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub others: Vec<Objective>,
}

/// `imsss:primaryObjective` or `imsss:objective`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Objective {
    /// `objectiveID`. Only a primary objective without maps may lack one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Default false.
    pub satisfied_by_measure: bool,
    /// From -1 to 1, default 1. Read when `satisfied_by_measure` is set.
    pub min_normalized_measure: f64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub map_info: Vec<MapInfo>,
}

/// `imsss:mapInfo`: how a local objective reads and writes a global one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct MapInfo {
    pub target_objective_id: String,
    /// Default true.
    pub read_satisfied_status: bool,
    /// Default true.
    pub read_normalized_measure: bool,
    /// Default false.
    pub write_satisfied_status: bool,
    /// Default false.
    pub write_normalized_measure: bool,
}

/// `adlseq:objective`: extends the objective of this activity with the same
/// id (which must exist).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct AdlObjective {
    pub id: String,
    pub map_info: Vec<AdlMapInfo>,
}

/// `adlseq:mapInfo`. Reads default true, writes default false.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct AdlMapInfo {
    pub target_objective_id: String,
    pub read_raw_score: bool,
    pub read_min_score: bool,
    pub read_max_score: bool,
    pub read_completion_status: bool,
    pub read_progress_measure: bool,
    pub write_raw_score: bool,
    pub write_min_score: bool,
    pub write_max_score: bool,
    pub write_completion_status: bool,
    pub write_progress_measure: bool,
}

/// `imsss:randomizationControls`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct RandomizationControls {
    /// Default `never`.
    pub randomization_timing: Timing,
    /// Children to select; absent means all of them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub select_count: Option<u32>,
    /// Default false.
    pub reorder_children: bool,
    /// Default `never`.
    pub selection_timing: Timing,
}

/// `imsss:deliveryControls`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct DeliveryControls {
    /// Default true.
    pub tracked: bool,
    /// Default false.
    pub completion_set_by_content: bool,
    /// Default false.
    pub objective_set_by_content: bool,
}

/// `adlseq:constrainedChoiceConsiderations`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ConstrainedChoiceConsiderations {
    /// Default false.
    pub prevent_activation: bool,
    /// Default false.
    pub constrain_choice: bool,
}

/// `adlseq:rollupConsiderations`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct RollupConsiderations {
    /// Default `always`, as are the other three.
    pub required_for_satisfied: RequiredFor,
    pub required_for_not_satisfied: RequiredFor,
    pub required_for_completed: RequiredFor,
    pub required_for_incomplete: RequiredFor,
    /// Default true.
    pub measure_satisfaction_if_active: bool,
}

/// `adlcp:completionThreshold`: when a lesson counts as complete by its
/// progress measure.
///
/// The 4th Edition states it in attributes. The 2nd and 3rd Editions state
/// only a number as the element's text, which meant "complete once progress
/// reaches it": that reads as `completed_by_measure` true with the number as
/// `min_progress_measure`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct CompletionThreshold {
    /// Default false.
    pub completed_by_measure: bool,
    /// From 0 to 1, default 1. The lesson's `cmi.completion_threshold`.
    pub min_progress_measure: f64,
    /// From 0 to 1, default 1. Weight in its parent's progress rollup.
    pub progress_weight: f64,
}

/// `adlcp:map` under `adlcp:data`: a shared data store the lesson reaches
/// through `adl.data` (4th Edition).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct SharedDataMap {
    pub target_id: String,
    /// Default true.
    pub read_shared_data: bool,
    /// Default true.
    pub write_shared_data: bool,
}

vocabulary! {
    /// How a rule's conditions combine.
    ConditionCombination { All = "all", Any = "any", }
}

vocabulary! {
    /// Whether a condition is taken as it is or negated.
    ConditionOperator { NoOp = "noOp", Not = "not", }
}

vocabulary! {
    /// What a sequencing rule condition tests.
    RuleConditionKind {
        Satisfied = "satisfied",
        ObjectiveStatusKnown = "objectiveStatusKnown",
        ObjectiveMeasureKnown = "objectiveMeasureKnown",
        ObjectiveMeasureGreaterThan = "objectiveMeasureGreaterThan",
        ObjectiveMeasureLessThan = "objectiveMeasureLessThan",
        Completed = "completed",
        ActivityProgressKnown = "activityProgressKnown",
        Attempted = "attempted",
        AttemptLimitExceeded = "attemptLimitExceeded",
        TimeLimitExceeded = "timeLimitExceeded",
        OutsideAvailableTimeRange = "outsideAvailableTimeRange",
        Always = "always",
    }
}

vocabulary! {
    /// What a sequencing rule does when its conditions hold.
    RuleAction {
        Skip = "skip",
        Disabled = "disabled",
        HiddenFromChoice = "hiddenFromChoice",
        StopForwardTraversal = "stopForwardTraversal",
        Exit = "exit",
        ExitParent = "exitParent",
        ExitAll = "exitAll",
        Retry = "retry",
        RetryAll = "retryAll",
        Continue = "continue",
        Previous = "previous",
    }
}

vocabulary! {
    /// Which children a rollup rule looks at.
    ChildActivitySet {
        All = "all",
        Any = "any",
        None = "none",
        AtLeastCount = "atLeastCount",
        AtLeastPercent = "atLeastPercent",
    }
}

vocabulary! {
    /// What a rollup rule condition tests on each child.
    RollupConditionKind {
        Satisfied = "satisfied",
        ObjectiveStatusKnown = "objectiveStatusKnown",
        ObjectiveMeasureKnown = "objectiveMeasureKnown",
        Completed = "completed",
        ActivityProgressKnown = "activityProgressKnown",
        Attempted = "attempted",
        AttemptLimitExceeded = "attemptLimitExceeded",
        TimeLimitExceeded = "timeLimitExceeded",
        OutsideAvailableTimeRange = "outsideAvailableTimeRange",
    }
}

vocabulary! {
    /// What a rollup rule sets on the parent.
    RollupAction {
        Satisfied = "satisfied",
        NotSatisfied = "notSatisfied",
        Completed = "completed",
        Incomplete = "incomplete",
    }
}

vocabulary! {
    /// When selection or randomisation happens.
    Timing { Never = "never", Once = "once", OnEachNewAttempt = "onEachNewAttempt", }
}

vocabulary! {
    /// When a child counts in its parent's rollup.
    RequiredFor {
        Always = "always",
        IfAttempted = "ifAttempted",
        IfNotSkipped = "ifNotSkipped",
        IfNotSuspended = "ifNotSuspended",
    }
}

vocabulary! {
    /// A navigation control the player draws, which a package may ask to
    /// hide (`adlnav:hideLMSUI`).
    NavigationControl {
        Previous = "previous",
        Continue = "continue",
        Exit = "exit",
        ExitAll = "exitAll",
        Abandon = "abandon",
        AbandonAll = "abandonAll",
        SuspendAll = "suspendAll",
    }
}
