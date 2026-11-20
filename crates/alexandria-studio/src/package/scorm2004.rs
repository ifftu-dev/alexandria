//! SCORM 2004 manifests, 2nd to 4th Edition: the shared content package
//! walk, plus each activity's sequencing, completion threshold, hidden
//! navigation controls and shared data maps, and the item elements the
//! run-time hands a lesson.
//!
//! Sequencing is read strictly: a value outside its vocabulary or range, a
//! rule without an action, a reference to an objective or collection that is
//! not there, is refused as [`RefusalCode::ManifestUnparseable`]. A sequencing
//! engine given such a definition would quietly do something else.
//! Elements SCORM 2004 tells a player to ignore are not read.

use std::collections::{HashMap, HashSet};
use std::io::{Read, Seek};

use super::archive::Archive;
use super::scorm::{invalid, parse_content_package, Version};
use super::sequencing::{
    AdlMapInfo, AdlObjective, ChildActivitySet, CompletionThreshold, ConditionCombination,
    ConditionOperator, ConstrainedChoiceConsiderations, ControlMode, DeliveryControls,
    LimitConditions, MapInfo, NavigationControl, Objective, Objectives, RandomizationControls,
    RequiredFor, RollupAction, RollupCondition, RollupConditionKind, RollupConsiderations,
    RollupRule, RollupRules, RuleAction, RuleCondition, RuleConditionKind, Sequencing,
    SequencingRule, SequencingRules, SharedDataMap, Timing, Vocabulary,
};
use super::xml::Element;
use super::{Activity, ParsedManifest, Refusal, RefusalCode};

/// The ADL sequencing extension namespace. Its `objectives` element sits
/// beside IMS's own inside `imsss:sequencing` and must not be mistaken for it.
const ADLSEQ: &str = "http://www.adlnet.org/xsd/adlseq_v1p3";

pub(crate) fn parse<R: Read + Seek>(
    manifest: &Element,
    archive: &Archive<R>,
) -> Result<ParsedManifest, Refusal> {
    if let Some(declared) = unknown_edition(manifest) {
        return Err(Refusal::new(
            RefusalCode::UnsupportedStandard,
            format!("the manifest declares {declared:?}, an edition of SCORM 2004 this build does not know"),
        ));
    }
    let mut reader = Scorm2004 {
        collections: collections(manifest)?,
    };
    parse_content_package(manifest, archive, &mut reader)
}

/// The declared schema version, when it names an edition other than the
/// three this build reads. A manifest that names none (`CAM 1.3` aside, which
/// is the 2nd Edition) is read as SCORM 2004 without an edition.
fn unknown_edition(manifest: &Element) -> Option<String> {
    let declared = manifest.child("metadata")?.child_text("schemaversion")?;
    let lower = declared.to_ascii_lowercase();
    (lower.contains("edition") && !["2nd", "3rd", "4th"].iter().any(|e| lower.contains(e)))
        .then(|| declared.to_owned())
}

/// Every `imsss:sequencingCollection` entry, by its `ID`.
fn collections(manifest: &Element) -> Result<HashMap<String, Sequencing>, Refusal> {
    let mut found = HashMap::new();
    for collection in manifest.children_named("sequencingCollection") {
        for sequencing in collection.children_named("sequencing") {
            let id = sequencing
                .attribute("ID")
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .ok_or_else(|| invalid("a <sequencing> in the sequencing collection has no ID"))?;
            let parsed = definition(sequencing)
                .map_err(|detail| invalid(format!("sequencing collection {id}: {detail}")))?;
            if found.insert(id.to_owned(), parsed).is_some() {
                return Err(invalid(format!(
                    "the sequencing collection uses the ID {id} more than once"
                )));
            }
        }
    }
    Ok(found)
}

struct Scorm2004 {
    collections: HashMap<String, Sequencing>,
}

impl Version for Scorm2004 {
    fn read(&mut self, element: &Element, activity: &mut Activity) -> Result<(), Refusal> {
        self.read_activity(element, activity)
            .map_err(|detail| invalid(format!("activity {}: {detail}", activity.id)))
    }
}

/// What a reader returns before the activity is named in the message.
type Read2004<T> = Result<T, String>;

impl Scorm2004 {
    fn read_activity(&self, element: &Element, activity: &mut Activity) -> Read2004<()> {
        activity.sequencing = match element.child("sequencing") {
            None => None,
            Some(sequencing) => Some(self.sequencing(sequencing)?),
        };
        activity.completion_threshold = element
            .child("completionThreshold")
            .map(completion_threshold)
            .transpose()?;
        activity.hide_lms_ui = hide_lms_ui(element)?;
        activity.shared_data = shared_data(element)?;

        let sequencing = activity.sequencing.as_ref();
        let Some(launch) = activity.launch.as_mut() else {
            return Ok(());
        };
        launch.launch_data = element.child_text("dataFromLMS").map(str::to_owned);
        launch.time_limit_action = element.child_text("timeLimitAction").map(str::to_owned);
        launch.max_time_allowed = sequencing
            .and_then(|s| s.limit_conditions.as_ref())
            .and_then(|limits| limits.attempt_absolute_duration_limit.clone());
        launch.mastery_score = sequencing
            .and_then(|s| s.objectives.as_ref())
            .map(|objectives| &objectives.primary)
            .filter(|primary| primary.satisfied_by_measure)
            .map(|primary| primary.min_normalized_measure);
        Ok(())
    }

    /// An item's own sequencing with its collection applied, checked whole.
    fn sequencing(&self, element: &Element) -> Read2004<Sequencing> {
        let own = definition(element)?;
        let mut sequencing = match element.attribute("IDRef").map(str::trim) {
            None | Some("") => own,
            Some(reference) => {
                let collection = self.collections.get(reference).ok_or_else(|| {
                    format!("refers to sequencing collection {reference}, which the manifest does not define")
                })?;
                applied(own, collection, reference)
            }
        };
        check_references(&mut sequencing)?;
        Ok(sequencing)
    }
}

/// The item's elements, then the collection's for every element the item
/// does not define (the content aggregation model's merge, element by
/// element).
fn applied(own: Sequencing, collection: &Sequencing, reference: &str) -> Sequencing {
    let from = collection.clone();
    Sequencing {
        collection: Some(reference.to_owned()),
        control_mode: own.control_mode.or(from.control_mode),
        sequencing_rules: own.sequencing_rules.or(from.sequencing_rules),
        limit_conditions: own.limit_conditions.or(from.limit_conditions),
        rollup_rules: own.rollup_rules.or(from.rollup_rules),
        objectives: own.objectives.or(from.objectives),
        adl_objectives: if own.adl_objectives.is_empty() {
            from.adl_objectives
        } else {
            own.adl_objectives
        },
        randomization_controls: own.randomization_controls.or(from.randomization_controls),
        delivery_controls: own.delivery_controls.or(from.delivery_controls),
        constrained_choice_considerations: own
            .constrained_choice_considerations
            .or(from.constrained_choice_considerations),
        rollup_considerations: own.rollup_considerations.or(from.rollup_considerations),
    }
}

/// Objective ids are unique, and every rule and ADL objective names one that
/// exists. Ids are URIs and are kept the way the test suite compares and
/// reports them: escapes decoded, runs of white space collapsed (OB-02a
/// writes `" %20obj%20%201%20 "` for `obj%201`; OB-12a then reads the
/// objective back as `ob j 1`). Global objective ids get the same treatment,
/// so the run-time compares plain strings.
fn check_references(sequencing: &mut Sequencing) -> Read2004<()> {
    let mut ids = HashSet::new();
    if let Some(objectives) = &mut sequencing.objectives {
        let all = std::iter::once(&mut objectives.primary).chain(&mut objectives.others);
        for objective in all {
            for map in &mut objective.map_info {
                map.target_objective_id = uri_key(&map.target_objective_id);
            }
            let Some(id) = objective.id.as_mut() else {
                continue;
            };
            *id = uri_key(id);
            if !ids.insert(id.clone()) {
                return Err(format!("the objective {id} is defined more than once"));
            }
        }
    }
    let rules = sequencing.sequencing_rules.iter_mut().flat_map(|rules| {
        rules
            .pre_condition_rules
            .iter_mut()
            .chain(&mut rules.exit_condition_rules)
            .chain(&mut rules.post_condition_rules)
    });
    for condition in rules.flat_map(|rule| &mut rule.conditions) {
        if let Some(objective) = condition.referenced_objective.as_mut() {
            *objective = uri_key(objective);
            if !ids.contains(objective.as_str()) {
                return Err(format!(
                    "a rule condition refers to objective {objective}, which the activity does not define"
                ));
            }
        }
    }
    for objective in &mut sequencing.adl_objectives {
        objective.id = uri_key(&objective.id);
        if !ids.contains(objective.id.as_str()) {
            return Err(format!(
                "adlseq:objective {} extends an objective the activity does not define",
                objective.id
            ));
        }
        for map in &mut objective.map_info {
            map.target_objective_id = uri_key(&map.target_objective_id);
        }
    }
    Ok(())
}

/// What two objective ids are compared by: escapes decoded, white space
/// collapsed to single spaces and trimmed.
fn uri_key(id: &str) -> String {
    let decoded = percent_encoding::percent_decode_str(id).decode_utf8_lossy();
    decoded.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// One `imsss:sequencing` element as written, collection not applied.
fn definition(element: &Element) -> Read2004<Sequencing> {
    let imsss_objectives = element
        .children_named("objectives")
        .find(|objectives| objectives.ns.as_deref() != Some(ADLSEQ));
    let adl_objectives = element
        .children_named("objectives")
        .find(|objectives| objectives.ns.as_deref() == Some(ADLSEQ));
    Ok(Sequencing {
        collection: None,
        control_mode: element.child("controlMode").map(control_mode).transpose()?,
        sequencing_rules: element
            .child("sequencingRules")
            .map(sequencing_rules)
            .transpose()?,
        limit_conditions: element
            .child("limitConditions")
            .map(limit_conditions)
            .transpose()?,
        rollup_rules: element.child("rollupRules").map(rollup_rules).transpose()?,
        objectives: imsss_objectives.map(objectives).transpose()?,
        adl_objectives: adl_objectives
            .map(adl_objective_list)
            .transpose()?
            .unwrap_or_default(),
        randomization_controls: element
            .child("randomizationControls")
            .map(randomization_controls)
            .transpose()?,
        delivery_controls: element
            .child("deliveryControls")
            .map(delivery_controls)
            .transpose()?,
        constrained_choice_considerations: element
            .child("constrainedChoiceConsiderations")
            .map(|e| {
                Ok::<_, String>(ConstrainedChoiceConsiderations {
                    prevent_activation: flag(e, "preventActivation", false)?,
                    constrain_choice: flag(e, "constrainChoice", false)?,
                })
            })
            .transpose()?,
        rollup_considerations: element
            .child("rollupConsiderations")
            .map(rollup_considerations)
            .transpose()?,
    })
}

fn control_mode(e: &Element) -> Read2004<ControlMode> {
    Ok(ControlMode {
        choice: flag(e, "choice", true)?,
        choice_exit: flag(e, "choiceExit", true)?,
        flow: flag(e, "flow", false)?,
        forward_only: flag(e, "forwardOnly", false)?,
        use_current_attempt_objective_info: flag(e, "useCurrentAttemptObjectiveInfo", true)?,
        use_current_attempt_progress_info: flag(e, "useCurrentAttemptProgressInfo", true)?,
    })
}

fn sequencing_rules(e: &Element) -> Read2004<SequencingRules> {
    use RuleAction::*;
    Ok(SequencingRules {
        pre_condition_rules: rules(
            e,
            "preConditionRule",
            &[Skip, Disabled, HiddenFromChoice, StopForwardTraversal],
        )?,
        exit_condition_rules: rules(e, "exitConditionRule", &[Exit])?,
        post_condition_rules: rules(
            e,
            "postConditionRule",
            &[ExitParent, ExitAll, Retry, RetryAll, Continue, Previous],
        )?,
    })
}

fn rules(e: &Element, kind: &str, allowed: &[RuleAction]) -> Read2004<Vec<SequencingRule>> {
    e.children_named(kind)
        .map(|rule| {
            let conditions = rule
                .child("ruleConditions")
                .ok_or_else(|| format!("a <{kind}> has no <ruleConditions>"))?;
            let list = conditions
                .children_named("ruleCondition")
                .map(rule_condition)
                .collect::<Read2004<Vec<_>>>()?;
            if list.is_empty() {
                return Err(format!("a <{kind}> has no <ruleCondition>"));
            }
            let action = rule
                .child("ruleAction")
                .ok_or_else(|| format!("a <{kind}> has no <ruleAction>"))?;
            let action: RuleAction = required_word(action, "action")?;
            if !allowed.contains(&action) {
                return Err(format!(
                    "a <{kind}> has the action {:?}, which only another kind of rule may have",
                    action.as_str()
                ));
            }
            Ok(SequencingRule {
                condition_combination: word(
                    conditions,
                    "conditionCombination",
                    ConditionCombination::All,
                )?,
                conditions: list,
                action,
            })
        })
        .collect()
}

fn rule_condition(e: &Element) -> Read2004<RuleCondition> {
    let condition: RuleConditionKind = required_word(e, "condition")?;
    let measure_threshold = optional_decimal(e, "measureThreshold", -1.0..=1.0)?;
    let compares_measure = matches!(
        condition,
        RuleConditionKind::ObjectiveMeasureGreaterThan
            | RuleConditionKind::ObjectiveMeasureLessThan
    );
    Ok(RuleCondition {
        condition,
        operator: word(e, "operator", ConditionOperator::NoOp)?,
        referenced_objective: text_attribute(e, "referencedObjective"),
        measure_threshold: measure_threshold.or(compares_measure.then_some(0.0)),
    })
}

fn limit_conditions(e: &Element) -> Read2004<LimitConditions> {
    let duration = text_attribute(e, "attemptAbsoluteDurationLimit");
    if let Some(duration) = &duration {
        if !is_duration(duration) {
            return Err(format!(
                "attemptAbsoluteDurationLimit {duration:?} is not a duration such as PT30M"
            ));
        }
    }
    Ok(LimitConditions {
        attempt_limit: count(e, "attemptLimit")?,
        attempt_absolute_duration_limit: duration,
    })
}

fn rollup_rules(e: &Element) -> Read2004<RollupRules> {
    let rules = e
        .children_named("rollupRule")
        .map(|rule| {
            let conditions = rule
                .child("rollupConditions")
                .ok_or("a <rollupRule> has no <rollupConditions>")?;
            let list = conditions
                .children_named("rollupCondition")
                .map(|condition| {
                    Ok(RollupCondition {
                        condition: required_word::<RollupConditionKind>(condition, "condition")?,
                        operator: word(condition, "operator", ConditionOperator::NoOp)?,
                    })
                })
                .collect::<Read2004<Vec<_>>>()?;
            if list.is_empty() {
                return Err("a <rollupRule> has no <rollupCondition>".to_owned());
            }
            let action = rule
                .child("rollupAction")
                .ok_or("a <rollupRule> has no <rollupAction>")?;
            Ok(RollupRule {
                child_activity_set: word(rule, "childActivitySet", ChildActivitySet::All)?,
                minimum_count: count(rule, "minimumCount")?.unwrap_or(0),
                minimum_percent: decimal(rule, "minimumPercent", 0.0, 0.0..=1.0)?,
                condition_combination: word(
                    conditions,
                    "conditionCombination",
                    ConditionCombination::Any,
                )?,
                conditions: list,
                action: required_word::<RollupAction>(action, "action")?,
            })
        })
        .collect::<Read2004<Vec<_>>>()?;
    Ok(RollupRules {
        rollup_objective_satisfied: flag(e, "rollupObjectiveSatisfied", true)?,
        rollup_progress_completion: flag(e, "rollupProgressCompletion", true)?,
        objective_measure_weight: decimal(e, "objectiveMeasureWeight", 1.0, 0.0..=1.0)?,
        rules,
    })
}

fn objectives(e: &Element) -> Read2004<Objectives> {
    let primary = e
        .child("primaryObjective")
        .ok_or("<objectives> has no <primaryObjective>")?;
    let primary = objective(primary)?;
    if primary.id.is_none() && !primary.map_info.is_empty() {
        return Err("the primary objective has maps but no objectiveID".to_owned());
    }
    let others = e
        .children_named("objective")
        .map(|other| {
            let other = objective(other)?;
            if other.id.is_none() {
                return Err("an <objective> has no objectiveID".to_owned());
            }
            Ok(other)
        })
        .collect::<Read2004<Vec<_>>>()?;
    Ok(Objectives { primary, others })
}

fn objective(e: &Element) -> Read2004<Objective> {
    let min_normalized_measure = match e.child_text("minNormalizedMeasure") {
        None => 1.0,
        Some(text) => in_range(text, "minNormalizedMeasure", -1.0..=1.0)?,
    };
    let map_info = e
        .children_named("mapInfo")
        .map(|map| {
            Ok(MapInfo {
                target_objective_id: required_text(map, "targetObjectiveID")?,
                read_satisfied_status: flag(map, "readSatisfiedStatus", true)?,
                read_normalized_measure: flag(map, "readNormalizedMeasure", true)?,
                write_satisfied_status: flag(map, "writeSatisfiedStatus", false)?,
                write_normalized_measure: flag(map, "writeNormalizedMeasure", false)?,
            })
        })
        .collect::<Read2004<Vec<_>>>()?;
    Ok(Objective {
        id: text_attribute(e, "objectiveID"),
        satisfied_by_measure: flag(e, "satisfiedByMeasure", false)?,
        min_normalized_measure,
        map_info,
    })
}

fn adl_objective_list(e: &Element) -> Read2004<Vec<AdlObjective>> {
    e.children_named("objective")
        .map(|objective| {
            let map_info = objective
                .children_named("mapInfo")
                .map(|map| {
                    Ok(AdlMapInfo {
                        target_objective_id: required_text(map, "targetObjectiveID")?,
                        read_raw_score: flag(map, "readRawScore", true)?,
                        read_min_score: flag(map, "readMinScore", true)?,
                        read_max_score: flag(map, "readMaxScore", true)?,
                        read_completion_status: flag(map, "readCompletionStatus", true)?,
                        read_progress_measure: flag(map, "readProgressMeasure", true)?,
                        write_raw_score: flag(map, "writeRawScore", false)?,
                        write_min_score: flag(map, "writeMinScore", false)?,
                        write_max_score: flag(map, "writeMaxScore", false)?,
                        write_completion_status: flag(map, "writeCompletionStatus", false)?,
                        write_progress_measure: flag(map, "writeProgressMeasure", false)?,
                    })
                })
                .collect::<Read2004<Vec<_>>>()?;
            Ok(AdlObjective {
                id: required_text(objective, "objectiveID")?,
                map_info,
            })
        })
        .collect()
}

fn randomization_controls(e: &Element) -> Read2004<RandomizationControls> {
    Ok(RandomizationControls {
        randomization_timing: word(e, "randomizationTiming", Timing::Never)?,
        select_count: count(e, "selectCount")?,
        reorder_children: flag(e, "reorderChildren", false)?,
        selection_timing: word(e, "selectionTiming", Timing::Never)?,
    })
}

fn delivery_controls(e: &Element) -> Read2004<DeliveryControls> {
    Ok(DeliveryControls {
        tracked: flag(e, "tracked", true)?,
        completion_set_by_content: flag(e, "completionSetByContent", false)?,
        objective_set_by_content: flag(e, "objectiveSetByContent", false)?,
    })
}

fn rollup_considerations(e: &Element) -> Read2004<RollupConsiderations> {
    Ok(RollupConsiderations {
        required_for_satisfied: word(e, "requiredForSatisfied", RequiredFor::Always)?,
        required_for_not_satisfied: word(e, "requiredForNotSatisfied", RequiredFor::Always)?,
        required_for_completed: word(e, "requiredForCompleted", RequiredFor::Always)?,
        required_for_incomplete: word(e, "requiredForIncomplete", RequiredFor::Always)?,
        measure_satisfaction_if_active: flag(e, "measureSatisfactionIfActive", true)?,
    })
}

fn completion_threshold(e: &Element) -> Read2004<CompletionThreshold> {
    let stated = ["completedByMeasure", "minProgressMeasure", "progressWeight"]
        .iter()
        .any(|name| e.attribute(name).is_some());
    let text = e.text.trim();
    if !stated && !text.is_empty() {
        // 2nd and 3rd Edition: the threshold is the element's text.
        return Ok(CompletionThreshold {
            completed_by_measure: true,
            min_progress_measure: in_range(text, "completionThreshold", 0.0..=1.0)?,
            progress_weight: 1.0,
        });
    }
    Ok(CompletionThreshold {
        completed_by_measure: flag(e, "completedByMeasure", false)?,
        min_progress_measure: decimal(e, "minProgressMeasure", 1.0, 0.0..=1.0)?,
        progress_weight: decimal(e, "progressWeight", 1.0, 0.0..=1.0)?,
    })
}

fn hide_lms_ui(item: &Element) -> Read2004<Vec<NavigationControl>> {
    let mut hidden = Vec::new();
    let interfaces = item
        .children_named("presentation")
        .flat_map(|presentation| presentation.children_named("navigationInterface"));
    for hide in interfaces.flat_map(|interface| interface.children_named("hideLMSUI")) {
        let control = NavigationControl::parse(&hide.text).ok_or_else(|| {
            format!(
                "hideLMSUI {:?} is not one of: {}",
                hide.text.trim(),
                NavigationControl::VALUES.trim_end()
            )
        })?;
        if !hidden.contains(&control) {
            hidden.push(control);
        }
    }
    Ok(hidden)
}

fn shared_data(item: &Element) -> Read2004<Vec<SharedDataMap>> {
    item.children_named("data")
        .flat_map(|data| data.children_named("map"))
        .map(|map| {
            Ok(SharedDataMap {
                target_id: required_text(map, "targetID")?,
                read_shared_data: flag(map, "readSharedData", true)?,
                write_shared_data: flag(map, "writeSharedData", true)?,
            })
        })
        .collect()
}

// ---- attribute values ------------------------------------------------------

/// A trimmed, non-empty attribute.
fn text_attribute(e: &Element, name: &str) -> Option<String> {
    e.attribute(name)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn required_text(e: &Element, name: &str) -> Read2004<String> {
    text_attribute(e, name).ok_or_else(|| format!("a <{}> has no {name}", e.name))
}

/// An `xs:boolean` attribute: `true`, `false`, `1` or `0`.
fn flag(e: &Element, name: &str, default: bool) -> Read2004<bool> {
    match e.attribute(name).map(str::trim) {
        None => Ok(default),
        Some(value) if value.eq_ignore_ascii_case("true") || value == "1" => Ok(true),
        Some(value) if value.eq_ignore_ascii_case("false") || value == "0" => Ok(false),
        Some(value) => Err(format!(
            "<{}> {name} is {value:?}; it must be true or false",
            e.name
        )),
    }
}

/// A non-negative whole number attribute.
fn count(e: &Element, name: &str) -> Read2004<Option<u32>> {
    let Some(value) = e.attribute(name).map(str::trim) else {
        return Ok(None);
    };
    value.parse::<u32>().map(Some).map_err(|_| {
        format!(
            "<{}> {name} is {value:?}; it must be a whole number",
            e.name
        )
    })
}

fn decimal(
    e: &Element,
    name: &str,
    default: f64,
    range: std::ops::RangeInclusive<f64>,
) -> Read2004<f64> {
    Ok(optional_decimal(e, name, range)?.unwrap_or(default))
}

fn optional_decimal(
    e: &Element,
    name: &str,
    range: std::ops::RangeInclusive<f64>,
) -> Read2004<Option<f64>> {
    e.attribute(name)
        .map(|value| in_range(value, name, range))
        .transpose()
}

fn in_range(text: &str, name: &str, range: std::ops::RangeInclusive<f64>) -> Read2004<f64> {
    let text = text.trim();
    text.parse::<f64>()
        .ok()
        .filter(|value| range.contains(value))
        .ok_or_else(|| {
            format!(
                "{name} is {text:?}; it must be a number from {} to {}",
                range.start(),
                range.end()
            )
        })
}

/// An attribute from a vocabulary, with the standard's default.
fn word<T: Vocabulary>(e: &Element, name: &str, default: T) -> Read2004<T> {
    match e.attribute(name) {
        None => Ok(default),
        Some(_) => required_word(e, name),
    }
}

fn required_word<T: Vocabulary>(e: &Element, name: &str) -> Read2004<T> {
    let value = e
        .attribute(name)
        .ok_or_else(|| format!("a <{}> has no {name}", e.name))?;
    T::parse(value).ok_or_else(|| {
        format!(
            "<{}> {name} {:?} is not one of: {}",
            e.name,
            value.trim(),
            T::VALUES.trim_end()
        )
    })
}

/// An XML Schema duration with no sign: `P`, then years, months and days,
/// then `T` and hours, minutes and seconds, each optional but at least one
/// present. Only seconds may have a fraction.
fn is_duration(text: &str) -> bool {
    fn parts(text: &str, units: &[char]) -> Option<usize> {
        let mut found = 0;
        let mut next_unit = 0;
        let mut number = String::new();
        for c in text.chars() {
            if c.is_ascii_digit() || c == '.' {
                number.push(c);
                continue;
            }
            let at = units[next_unit..].iter().position(|unit| *unit == c)? + next_unit;
            let fraction_ok = c == 'S';
            let valid = !number.is_empty()
                && !number.starts_with('.')
                && !number.ends_with('.')
                && number.matches('.').count() <= usize::from(fraction_ok);
            if !valid {
                return None;
            }
            number.clear();
            next_unit = at + 1;
            found += 1;
        }
        number.is_empty().then_some(found)
    }
    let Some(rest) = text.trim().strip_prefix('P') else {
        return false;
    };
    let (date, time) = match rest.split_once('T') {
        Some((date, time)) => (date, Some(time)),
        None => (rest, None),
    };
    let Some(date_parts) = parts(date, &['Y', 'M', 'D']) else {
        return false;
    };
    match time {
        None => date_parts > 0,
        Some(time) => parts(time, &['H', 'M', 'S']).is_some_and(|n| n > 0),
    }
}
