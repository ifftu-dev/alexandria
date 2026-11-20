//! SCORM 2004 import: a package from any of the three editions goes in, and
//! a description whose activity tree carries the manifest's sequencing comes
//! out, or a stable refusal code.
//!
//! Archives are built here with the zip crate's own writer. The hostile
//! archive cases are covered by `package_import.rs`; these packages are
//! well-formed zips and only their manifests vary.

use std::io::{Cursor, Write};

use alexandria_studio::package::{
    import_package, Edition, LaunchKind, Limits, PackageDescription, Refusal, RefusalCode, Standard,
};

/// A stored zip holding these files.
fn zip(files: &[(&str, &str)]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (name, data) in files {
        writer.start_file(*name, options).unwrap();
        writer.write_all(data.as_bytes()).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

const NAMESPACES: &str = r#"xmlns="http://www.imsglobal.org/xsd/imscp_v1p1"
    xmlns:adlcp="http://www.adlnet.org/xsd/adlcp_v1p3"
    xmlns:adlseq="http://www.adlnet.org/xsd/adlseq_v1p3"
    xmlns:adlnav="http://www.adlnet.org/xsd/adlnav_v1p3"
    xmlns:imsss="http://www.imsglobal.org/xsd/imsss""#;

/// A SCORM 2004 manifest: `schemaversion`, then the default organization's
/// contents, then anything that follows `<organizations>` (a sequencing
/// collection), then the resources.
fn manifest(schemaversion: &str, organization: &str, after: &str, resources: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<manifest identifier="com.example.sequenced" version="1" {NAMESPACES}>
  <metadata>
    <schema>ADL SCORM</schema>
    <schemaversion>{schemaversion}</schemaversion>
  </metadata>
  <organizations default="course">
    <organization identifier="course">
      {organization}
    </organization>
  </organizations>
  {after}
  <resources>
    {resources}
  </resources>
</manifest>"#
    )
}

/// Two lessons and a quiz, a resource each.
const RESOURCES: &str = r#"
    <resource identifier="r-intro" type="webcontent" adlcp:scormType="sco" href="intro.html"/>
    <resource identifier="r-body" type="webcontent" adlcp:scormType="sco" href="body.html"/>
    <resource identifier="r-quiz" type="webcontent" adlcp:scormType="sco" href="quiz.html"/>
    <resource identifier="r-sheet" type="webcontent" adlcp:scormType="asset" href="sheet.html"/>"#;

const FILES: [&str; 4] = ["intro.html", "body.html", "quiz.html", "sheet.html"];

fn import_manifest(manifest: &str) -> Result<PackageDescription, Refusal> {
    let mut files = vec![("imsmanifest.xml", manifest)];
    files.extend(FILES.iter().map(|name| (*name, "<html></html>")));
    import_package(Cursor::new(zip(&files)), &Limits::default())
}

fn import(manifest: &str) -> PackageDescription {
    import_manifest(manifest).unwrap_or_else(|refusal| panic!("refused: {refusal}"))
}

fn refusal(manifest: &str) -> Refusal {
    import_manifest(manifest).expect_err("the package should be refused")
}

/// A plain two-lesson organization with no sequencing at all.
const PLAIN: &str = r#"
      <title>Plain course</title>
      <item identifier="intro" identifierref="r-intro"><title>Introduction</title></item>
      <item identifier="module" isvisible="false">
        <title>Module</title>
        <item identifier="body" identifierref="r-body" parameters="?part=1"><title>Body</title></item>
        <item identifier="sheet" identifierref="r-sheet"><title>Sheet</title></item>
      </item>"#;

#[test]
fn a_scorm2004_package_from_each_edition_imports_with_its_edition() {
    let editions = [
        ("CAM 1.3", Some(Edition::Second)),
        ("2004 3rd Edition", Some(Edition::Third)),
        ("2004 4th Edition", Some(Edition::Fourth)),
    ];
    for (schemaversion, edition) in editions {
        let package = import(&manifest(schemaversion, PLAIN, "", RESOURCES));
        assert_eq!(package.standard, Standard::Scorm2004, "{schemaversion}");
        assert_eq!(package.edition, edition, "{schemaversion}");
        assert_eq!(package.identifier, "com.example.sequenced");
        assert_eq!(package.title, "Plain course");

        let tree = &package.tree;
        assert_eq!(tree.id, "course");
        assert!(tree.launch.is_none());
        let ids: Vec<&str> = tree.children.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, ["intro", "module"]);
        let module = &tree.children[1];
        assert!(!module.visible);
        assert!(module.launch.is_none());

        let body = module.children[0].launch.as_ref().unwrap();
        assert_eq!(body.path, "body.html");
        assert_eq!(body.parameters.as_deref(), Some("?part=1"));
        assert_eq!(body.kind, LaunchKind::Sco);
        let sheet = module.children[1].launch.as_ref().unwrap();
        assert_eq!(sheet.kind, LaunchKind::Asset);

        let launchable: Vec<&str> = package
            .launchable_activities()
            .iter()
            .map(|a| a.id.as_str())
            .collect();
        assert_eq!(launchable, ["intro", "body", "sheet"]);
    }
}

/// A 4th Edition course using every part of sequencing the description
/// carries: controls on the organization, a cluster with rules,
/// randomisation, rollup and ADL considerations, a lesson that takes most of
/// its sequencing from a collection and overrides one element, objectives
/// with global maps, a completion threshold, shared data and hidden
/// navigation controls.
const FOURTH: &str = r#"
      <title>Sequenced course</title>
      <item identifier="intro" identifierref="r-intro">
        <title>Introduction</title>
        <adlcp:dataFromLMS>region=emea</adlcp:dataFromLMS>
        <adlcp:timeLimitAction>exit,message</adlcp:timeLimitAction>
        <imsss:sequencing>
          <imsss:deliveryControls tracked="false" objectiveSetByContent="true"/>
        </imsss:sequencing>
        <adlnav:presentation>
          <adlnav:navigationInterface>
            <adlnav:hideLMSUI>continue</adlnav:hideLMSUI>
            <adlnav:hideLMSUI>previous</adlnav:hideLMSUI>
          </adlnav:navigationInterface>
        </adlnav:presentation>
      </item>
      <item identifier="module">
        <title>Module</title>
        <item identifier="body" identifierref="r-body">
          <title>Body</title>
          <adlcp:completionThreshold completedByMeasure="true" minProgressMeasure="0.8" progressWeight="0.5"/>
          <imsss:sequencing IDRef="lesson-defaults">
            <imsss:sequencingRules>
              <imsss:postConditionRule>
                <imsss:ruleConditions>
                  <imsss:ruleCondition referencedObjective="extra" measureThreshold="0.5"
                      condition="objectiveMeasureGreaterThan"/>
                </imsss:ruleConditions>
                <imsss:ruleAction action="exitParent"/>
              </imsss:postConditionRule>
            </imsss:sequencingRules>
            <imsss:limitConditions attemptLimit="3" attemptAbsoluteDurationLimit="PT30M"/>
            <imsss:objectives>
              <imsss:primaryObjective objectiveID="body-pass" satisfiedByMeasure="true">
                <imsss:minNormalizedMeasure>0.6</imsss:minNormalizedMeasure>
                <imsss:mapInfo targetObjectiveID="g-body" writeSatisfiedStatus="true"/>
              </imsss:primaryObjective>
              <imsss:objective objectiveID="extra"/>
            </imsss:objectives>
            <adlseq:objectives>
              <adlseq:objective objectiveID="body-pass">
                <adlseq:mapInfo targetObjectiveID="g-body" writeRawScore="true" readProgressMeasure="false"/>
              </adlseq:objective>
            </adlseq:objectives>
          </imsss:sequencing>
          <adlcp:data>
            <adlcp:map targetID="notes" writeSharedData="false"/>
          </adlcp:data>
        </item>
        <item identifier="sheet" identifierref="r-sheet"><title>Sheet</title></item>
        <imsss:sequencing>
          <imsss:controlMode choice="false" flow="true" forwardOnly="true"/>
          <imsss:sequencingRules>
            <imsss:preConditionRule>
              <imsss:ruleConditions conditionCombination="any">
                <imsss:ruleCondition condition="satisfied" operator="not"/>
                <imsss:ruleCondition condition="attemptLimitExceeded"/>
              </imsss:ruleConditions>
              <imsss:ruleAction action="disabled"/>
            </imsss:preConditionRule>
            <imsss:exitConditionRule>
              <imsss:ruleConditions><imsss:ruleCondition condition="completed"/></imsss:ruleConditions>
              <imsss:ruleAction action="exit"/>
            </imsss:exitConditionRule>
          </imsss:sequencingRules>
          <imsss:rollupRules rollupProgressCompletion="false" objectiveMeasureWeight="0.25">
            <imsss:rollupRule childActivitySet="atLeastCount" minimumCount="1">
              <imsss:rollupConditions>
                <imsss:rollupCondition condition="satisfied"/>
              </imsss:rollupConditions>
              <imsss:rollupAction action="satisfied"/>
            </imsss:rollupRule>
            <imsss:rollupRule childActivitySet="atLeastPercent" minimumPercent="0.5">
              <imsss:rollupConditions conditionCombination="all">
                <imsss:rollupCondition condition="attempted"/>
                <imsss:rollupCondition operator="not" condition="completed"/>
              </imsss:rollupConditions>
              <imsss:rollupAction action="incomplete"/>
            </imsss:rollupRule>
          </imsss:rollupRules>
          <imsss:randomizationControls randomizationTiming="onEachNewAttempt" reorderChildren="true"
              selectCount="1" selectionTiming="once"/>
          <adlseq:constrainedChoiceConsiderations constrainChoice="true"/>
          <adlseq:rollupConsiderations requiredForSatisfied="ifAttempted" measureSatisfactionIfActive="false"/>
        </imsss:sequencing>
      </item>
      <imsss:sequencing>
        <imsss:controlMode choice="false" flow="true"/>
      </imsss:sequencing>"#;

const COLLECTION: &str = r#"
  <imsss:sequencingCollection>
    <imsss:sequencing ID="lesson-defaults">
      <imsss:controlMode choiceExit="false"/>
      <imsss:limitConditions attemptLimit="1"/>
      <imsss:deliveryControls completionSetByContent="true"/>
    </imsss:sequencing>
  </imsss:sequencingCollection>"#;

fn json<T: serde::Serialize>(value: &T) -> serde_json::Value {
    serde_json::to_value(value).unwrap()
}

#[test]
fn a_4th_edition_package_keeps_its_sequencing_on_each_activity() {
    let package = import(&manifest("2004 4th Edition", FOURTH, COLLECTION, RESOURCES));
    assert_eq!(package.edition, Some(Edition::Fourth));

    let root = &package.tree;
    assert_eq!(
        json(&root.sequencing),
        serde_json::json!({
            "control_mode": {
                "choice": false, "choice_exit": true, "flow": true, "forward_only": false,
                "use_current_attempt_objective_info": true,
                "use_current_attempt_progress_info": true,
            },
        })
    );

    let intro = &root.children[0];
    assert_eq!(
        json(&intro.sequencing),
        serde_json::json!({
            "delivery_controls": {
                "tracked": false, "completion_set_by_content": false,
                "objective_set_by_content": true,
            },
        })
    );
    assert_eq!(
        json(&intro.hide_lms_ui),
        serde_json::json!(["continue", "previous"])
    );
    let launch = intro.launch.as_ref().unwrap();
    assert_eq!(launch.launch_data.as_deref(), Some("region=emea"));
    assert_eq!(launch.time_limit_action.as_deref(), Some("exit,message"));
    assert_eq!(launch.max_time_allowed, None);
    assert_eq!(launch.mastery_score, None);

    let module = &root.children[1];
    assert!(module.launch.is_none());
    assert_eq!(
        json(&module.sequencing),
        serde_json::json!({
            "control_mode": {
                "choice": false, "choice_exit": true, "flow": true, "forward_only": true,
                "use_current_attempt_objective_info": true,
                "use_current_attempt_progress_info": true,
            },
            "sequencing_rules": {
                "pre_condition_rules": [{
                    "condition_combination": "any",
                    "conditions": [
                        { "condition": "satisfied", "operator": "not" },
                        { "condition": "attemptLimitExceeded", "operator": "noOp" },
                    ],
                    "action": "disabled",
                }],
                "exit_condition_rules": [{
                    "condition_combination": "all",
                    "conditions": [{ "condition": "completed", "operator": "noOp" }],
                    "action": "exit",
                }],
            },
            "rollup_rules": {
                "rollup_objective_satisfied": true,
                "rollup_progress_completion": false,
                "objective_measure_weight": 0.25,
                "rules": [
                    {
                        "child_activity_set": "atLeastCount", "minimum_count": 1,
                        "minimum_percent": 0.0, "condition_combination": "any",
                        "conditions": [{ "condition": "satisfied", "operator": "noOp" }],
                        "action": "satisfied",
                    },
                    {
                        "child_activity_set": "atLeastPercent", "minimum_count": 0,
                        "minimum_percent": 0.5, "condition_combination": "all",
                        "conditions": [
                            { "condition": "attempted", "operator": "noOp" },
                            { "condition": "completed", "operator": "not" },
                        ],
                        "action": "incomplete",
                    },
                ],
            },
            "randomization_controls": {
                "randomization_timing": "onEachNewAttempt", "select_count": 1,
                "reorder_children": true, "selection_timing": "once",
            },
            "constrained_choice_considerations": {
                "prevent_activation": false, "constrain_choice": true,
            },
            "rollup_considerations": {
                "required_for_satisfied": "ifAttempted",
                "required_for_not_satisfied": "always",
                "required_for_completed": "always",
                "required_for_incomplete": "always",
                "measure_satisfaction_if_active": false,
            },
        })
    );

    let body = &module.children[0];
    assert_eq!(
        json(&body.sequencing),
        serde_json::json!({
            "collection": "lesson-defaults",
            // From the collection: the item does not define these.
            "control_mode": {
                "choice": true, "choice_exit": false, "flow": false, "forward_only": false,
                "use_current_attempt_objective_info": true,
                "use_current_attempt_progress_info": true,
            },
            "delivery_controls": {
                "tracked": true, "completion_set_by_content": true,
                "objective_set_by_content": false,
            },
            // The item's own, replacing the collection's attempt limit of 1.
            "limit_conditions": { "attempt_limit": 3, "attempt_absolute_duration_limit": "PT30M" },
            "sequencing_rules": {
                "post_condition_rules": [{
                    "condition_combination": "all",
                    "conditions": [{
                        "condition": "objectiveMeasureGreaterThan", "operator": "noOp",
                        "referenced_objective": "extra", "measure_threshold": 0.5,
                    }],
                    "action": "exitParent",
                }],
            },
            "objectives": {
                "primary": {
                    "id": "body-pass", "satisfied_by_measure": true,
                    "min_normalized_measure": 0.6,
                    "map_info": [{
                        "target_objective_id": "g-body",
                        "read_satisfied_status": true, "read_normalized_measure": true,
                        "write_satisfied_status": true, "write_normalized_measure": false,
                    }],
                },
                "others": [{ "id": "extra", "satisfied_by_measure": false, "min_normalized_measure": 1.0 }],
            },
            "adl_objectives": [{
                "id": "body-pass",
                "map_info": [{
                    "target_objective_id": "g-body",
                    "read_raw_score": true, "read_min_score": true, "read_max_score": true,
                    "read_completion_status": true, "read_progress_measure": false,
                    "write_raw_score": true, "write_min_score": false, "write_max_score": false,
                    "write_completion_status": false, "write_progress_measure": false,
                }],
            }],
        })
    );
    assert_eq!(
        json(&body.completion_threshold),
        serde_json::json!({ "completed_by_measure": true, "min_progress_measure": 0.8, "progress_weight": 0.5 })
    );
    assert_eq!(
        json(&body.shared_data),
        serde_json::json!([{ "target_id": "notes", "read_shared_data": true, "write_shared_data": false }])
    );
    // What the run-time is handed when the lesson starts.
    let launch = body.launch.as_ref().unwrap();
    assert_eq!(launch.max_time_allowed.as_deref(), Some("PT30M"));
    assert_eq!(launch.mastery_score, Some(0.6));

    // The asset has no sequencing of its own.
    let sheet = &module.children[1];
    assert!(sheet.sequencing.is_none());
    assert!(sheet.completion_threshold.is_none());
}

#[test]
fn a_sequenced_description_survives_being_stored_as_json() {
    let package = import(&manifest("2004 4th Edition", FOURTH, COLLECTION, RESOURCES));
    let stored = serde_json::to_string(&package).unwrap();
    let restored: PackageDescription = serde_json::from_str(&stored).unwrap();
    assert_eq!(restored, package);
    assert_eq!(json(&restored)["edition"], "4th");
    assert_eq!(json(&restored)["standard"], "scorm2004");
}

#[test]
fn a_3rd_edition_package_keeps_its_sequencing_and_its_text_completion_threshold() {
    let organization = r#"
      <title>Third</title>
      <item identifier="intro" identifierref="r-intro">
        <title>Introduction</title>
        <adlcp:completionThreshold>0.75</adlcp:completionThreshold>
        <imsss:sequencing>
          <imsss:sequencingRules>
            <imsss:preConditionRule>
              <imsss:ruleConditions>
                <imsss:ruleCondition referencedObjective="intro-done" condition="objectiveMeasureLessThan"/>
              </imsss:ruleConditions>
              <imsss:ruleAction action="hiddenFromChoice"/>
            </imsss:preConditionRule>
          </imsss:sequencingRules>
          <imsss:objectives>
            <imsss:primaryObjective objectiveID="intro-done">
              <imsss:mapInfo targetObjectiveID="global-intro"/>
            </imsss:primaryObjective>
          </imsss:objectives>
        </imsss:sequencing>
      </item>
      <imsss:sequencing>
        <imsss:controlMode flow="true"/>
        <imsss:rollupRules rollupObjectiveSatisfied="false"/>
      </imsss:sequencing>"#;
    let package = import(&manifest("2004 3rd Edition", organization, "", RESOURCES));
    assert_eq!(package.edition, Some(Edition::Third));

    let intro = &package.tree.children[0];
    // "Complete once progress reaches 0.75", as the 4th Edition would say it.
    assert_eq!(
        json(&intro.completion_threshold),
        serde_json::json!({ "completed_by_measure": true, "min_progress_measure": 0.75, "progress_weight": 1.0 })
    );
    assert_eq!(
        json(&intro.sequencing),
        serde_json::json!({
            "sequencing_rules": {
                "pre_condition_rules": [{
                    "condition_combination": "all",
                    "conditions": [{
                        "condition": "objectiveMeasureLessThan", "operator": "noOp",
                        "referenced_objective": "intro-done",
                        // Stated by nobody: the standard's default for a comparison.
                        "measure_threshold": 0.0,
                    }],
                    "action": "hiddenFromChoice",
                }],
            },
            "objectives": {
                "primary": {
                    "id": "intro-done", "satisfied_by_measure": false,
                    "min_normalized_measure": 1.0,
                    "map_info": [{
                        "target_objective_id": "global-intro",
                        "read_satisfied_status": true, "read_normalized_measure": true,
                        "write_satisfied_status": false, "write_normalized_measure": false,
                    }],
                },
            },
        })
    );
    // Not satisfied by measure, so there is no passing score to hand over.
    assert_eq!(intro.launch.as_ref().unwrap().mastery_score, None);

    let root = package.tree.sequencing.as_ref().unwrap();
    assert!(root.control_mode.unwrap().flow);
    assert!(
        !root
            .rollup_rules
            .as_ref()
            .unwrap()
            .rollup_objective_satisfied
    );
    assert!(root.rollup_rules.as_ref().unwrap().rules.is_empty());
}

#[test]
fn a_2nd_edition_package_keeps_its_sequencing() {
    let organization = r#"
      <title>Second</title>
      <item identifier="intro" identifierref="r-intro">
        <title>Introduction</title>
        <imsss:sequencing>
          <imsss:sequencingRules>
            <imsss:postConditionRule>
              <imsss:ruleConditions>
                <imsss:ruleCondition condition="always"/>
              </imsss:ruleConditions>
              <imsss:ruleAction action="continue"/>
            </imsss:postConditionRule>
          </imsss:sequencingRules>
          <imsss:limitConditions attemptLimit="2"/>
        </imsss:sequencing>
      </item>
      <item identifier="quiz" identifierref="r-quiz">
        <title>Quiz</title>
        <imsss:sequencing>
          <imsss:objectives>
            <imsss:primaryObjective satisfiedByMeasure="1">
              <imsss:minNormalizedMeasure>-0.25</imsss:minNormalizedMeasure>
            </imsss:primaryObjective>
          </imsss:objectives>
          <imsss:deliveryControls completionSetByContent="True" objectiveSetByContent="1"/>
        </imsss:sequencing>
      </item>
      <imsss:sequencing>
        <imsss:controlMode choice="true" choiceExit="false" flow="true"/>
      </imsss:sequencing>"#;
    let package = import(&manifest("CAM 1.3", organization, "", RESOURCES));
    assert_eq!(package.edition, Some(Edition::Second));

    let root = package.tree.sequencing.as_ref().unwrap();
    let control = root.control_mode.unwrap();
    assert!(control.choice && !control.choice_exit && control.flow);

    let intro = package.tree.children[0].sequencing.as_ref().unwrap();
    let rule = &intro
        .sequencing_rules
        .as_ref()
        .unwrap()
        .post_condition_rules[0];
    assert_eq!(rule.action.as_str(), "continue");
    assert_eq!(rule.conditions[0].condition.as_str(), "always");
    assert_eq!(
        intro.limit_conditions.as_ref().unwrap().attempt_limit,
        Some(2)
    );

    let quiz = &package.tree.children[1];
    let sequencing = quiz.sequencing.as_ref().unwrap();
    let delivery = sequencing.delivery_controls.unwrap();
    assert!(
        delivery.tracked && delivery.completion_set_by_content && delivery.objective_set_by_content
    );
    let primary = &sequencing.objectives.as_ref().unwrap().primary;
    assert_eq!(primary.id, None);
    // Satisfied by measure: the minimum is the lesson's scaled passing score.
    assert_eq!(quiz.launch.as_ref().unwrap().mastery_score, Some(-0.25));
}

#[test]
fn a_scorm2004_manifest_that_names_no_edition_imports_without_one() {
    let package = import(&manifest("1.3", PLAIN, "", RESOURCES));
    assert_eq!(package.standard, Standard::Scorm2004);
    assert_eq!(package.edition, None);
}

// ---- refusals ----------------------------------------------------------------

/// A 4th Edition package whose one lesson carries `inside` (sequencing,
/// presentation, completion threshold), with `after` following the
/// organizations.
fn lesson_with(inside: &str, after: &str) -> String {
    let organization = format!(
        r#"<title>Broken</title>
           <item identifier="intro" identifierref="r-intro"><title>Introduction</title>{inside}</item>"#
    );
    manifest("2004 4th Edition", &organization, after, RESOURCES)
}

fn rule(kind: &str, conditions: &str, action: &str) -> String {
    format!(
        r#"<imsss:sequencing><imsss:sequencingRules><imsss:{kind}>
             <imsss:ruleConditions>{conditions}</imsss:ruleConditions>{action}
           </imsss:{kind}></imsss:sequencingRules></imsss:sequencing>"#
    )
}

#[test]
fn malformed_sequencing_is_refused_as_unparseable() {
    let satisfied = r#"<imsss:ruleCondition condition="satisfied"/>"#;
    let skip = r#"<imsss:ruleAction action="skip"/>"#;
    let objectives = |inner: &str| {
        format!("<imsss:sequencing><imsss:objectives>{inner}</imsss:objectives></imsss:sequencing>")
    };
    let rollup = |rule: &str| {
        format!(
            "<imsss:sequencing><imsss:rollupRules>{rule}</imsss:rollupRules></imsss:sequencing>"
        )
    };
    let cases: Vec<(&str, String, &str)> = vec![
        (
            "a condition outside the vocabulary",
            rule("preConditionRule", r#"<imsss:ruleCondition condition="passed"/>"#, skip),
            "",
        ),
        ("a rule with no action", rule("preConditionRule", satisfied, ""), ""),
        ("a rule with no condition", rule("preConditionRule", "", skip), ""),
        (
            "an action the rule's kind may not have",
            rule("preConditionRule", satisfied, r#"<imsss:ruleAction action="exitAll"/>"#),
            "",
        ),
        (
            "a measure threshold past 1",
            rule(
                "preConditionRule",
                r#"<imsss:ruleCondition condition="objectiveMeasureGreaterThan" measureThreshold="1.5"/>"#,
                skip,
            ),
            "",
        ),
        (
            "a rule on an objective the activity does not define",
            rule(
                "preConditionRule",
                r#"<imsss:ruleCondition condition="satisfied" referencedObjective="nowhere"/>"#,
                skip,
            ),
            "",
        ),
        (
            "a negative attempt limit",
            r#"<imsss:sequencing><imsss:limitConditions attemptLimit="-1"/></imsss:sequencing>"#.into(),
            "",
        ),
        (
            "a duration limit that is not a duration",
            r#"<imsss:sequencing><imsss:limitConditions attemptAbsoluteDurationLimit="00:30:00"/></imsss:sequencing>"#.into(),
            "",
        ),
        (
            "a duration limit with a fraction of a minute",
            r#"<imsss:sequencing><imsss:limitConditions attemptAbsoluteDurationLimit="PT1.5M"/></imsss:sequencing>"#.into(),
            "",
        ),
        (
            "a control that is not true or false",
            r#"<imsss:sequencing><imsss:controlMode flow="yes"/></imsss:sequencing>"#.into(),
            "",
        ),
        (
            "a timing outside the vocabulary",
            r#"<imsss:sequencing><imsss:randomizationControls randomizationTiming="sometimes"/></imsss:sequencing>"#.into(),
            "",
        ),
        (
            "objectives with no primary objective",
            objectives(r#"<imsss:objective objectiveID="a"/>"#),
            "",
        ),
        (
            "an objective with no id",
            objectives(r#"<imsss:primaryObjective objectiveID="p"/><imsss:objective/>"#),
            "",
        ),
        (
            "two objectives with one id",
            objectives(r#"<imsss:primaryObjective objectiveID="p"/><imsss:objective objectiveID="p"/>"#),
            "",
        ),
        (
            "a minimum measure below -1",
            objectives(
                r#"<imsss:primaryObjective objectiveID="p"><imsss:minNormalizedMeasure>-2</imsss:minNormalizedMeasure></imsss:primaryObjective>"#,
            ),
            "",
        ),
        (
            "a map with no target",
            objectives(r#"<imsss:primaryObjective objectiveID="p"><imsss:mapInfo/></imsss:primaryObjective>"#),
            "",
        ),
        (
            "an ADL objective extending one the activity does not define",
            r#"<imsss:sequencing>
                 <imsss:objectives><imsss:primaryObjective objectiveID="p"/></imsss:objectives>
                 <adlseq:objectives><adlseq:objective objectiveID="q"/></adlseq:objectives>
               </imsss:sequencing>"#
                .into(),
            "",
        ),
        (
            "a rollup rule with no action",
            rollup(
                r#"<imsss:rollupRule><imsss:rollupConditions><imsss:rollupCondition condition="satisfied"/></imsss:rollupConditions></imsss:rollupRule>"#,
            ),
            "",
        ),
        (
            "a rollup condition only sequencing rules may test",
            rollup(
                r#"<imsss:rollupRule><imsss:rollupConditions><imsss:rollupCondition condition="always"/></imsss:rollupConditions><imsss:rollupAction action="satisfied"/></imsss:rollupRule>"#,
            ),
            "",
        ),
        (
            "a minimum percent written as a percentage",
            rollup(
                r#"<imsss:rollupRule childActivitySet="atLeastPercent" minimumPercent="50"><imsss:rollupConditions><imsss:rollupCondition condition="satisfied"/></imsss:rollupConditions><imsss:rollupAction action="satisfied"/></imsss:rollupRule>"#,
            ),
            "",
        ),
        (
            "a rollup consideration outside the vocabulary",
            r#"<imsss:sequencing><adlseq:rollupConsiderations requiredForSatisfied="sometimes"/></imsss:sequencing>"#.into(),
            "",
        ),
        (
            "a reference to a collection that is not there",
            r#"<imsss:sequencing IDRef="missing"/>"#.into(),
            "",
        ),
        (
            "two collection entries with one ID",
            r#"<imsss:sequencing IDRef="c"/>"#.into(),
            r#"<imsss:sequencingCollection>
                 <imsss:sequencing ID="c"/><imsss:sequencing ID="c"/>
               </imsss:sequencingCollection>"#,
        ),
        (
            "a malformed collection entry",
            String::new(),
            r#"<imsss:sequencingCollection>
                 <imsss:sequencing ID="c"><imsss:controlMode choice="maybe"/></imsss:sequencing>
               </imsss:sequencingCollection>"#,
        ),
        (
            "a navigation control outside the vocabulary",
            r#"<adlnav:presentation><adlnav:navigationInterface>
                 <adlnav:hideLMSUI>everything</adlnav:hideLMSUI>
               </adlnav:navigationInterface></adlnav:presentation>"#
                .into(),
            "",
        ),
        (
            "a completion threshold past 1",
            r#"<adlcp:completionThreshold minProgressMeasure="2"/>"#.into(),
            "",
        ),
        (
            "a text completion threshold that is not a number",
            r#"<adlcp:completionThreshold>most</adlcp:completionThreshold>"#.into(),
            "",
        ),
        (
            "a shared data map with no target",
            r#"<adlcp:data><adlcp:map readSharedData="true"/></adlcp:data>"#.into(),
            "",
        ),
    ];

    // The fixture itself imports: each refusal below is the case's doing.
    import(&lesson_with("", ""));
    for (case, inside, after) in cases {
        let refused = refusal(&lesson_with(&inside, after));
        assert_eq!(
            refused.code,
            RefusalCode::ManifestUnparseable,
            "{case}: {refused}"
        );
        assert_eq!(refused.standard, Some(Standard::Scorm2004), "{case}");
        assert_eq!(refused.edition, Some(Edition::Fourth), "{case}");
    }
}

#[test]
fn a_refusal_for_broken_sequencing_names_the_activity() {
    let refused = refusal(&lesson_with(
        r#"<imsss:sequencing><imsss:controlMode flow="yes"/></imsss:sequencing>"#,
        "",
    ));
    assert!(refused.detail.contains("intro"), "{}", refused.detail);
    assert!(refused.detail.contains("flow"), "{}", refused.detail);
}

#[test]
fn values_written_in_the_wrong_case_are_still_read() {
    let package = import(&lesson_with(
        &rule(
            "preConditionRule",
            r#"<imsss:ruleCondition condition="Satisfied" operator="NOT"/>"#,
            r#"<imsss:ruleAction action="Skip"/>"#,
        ),
        "",
    ));
    let sequencing = package.tree.children[0].sequencing.as_ref().unwrap();
    let rule = &sequencing
        .sequencing_rules
        .as_ref()
        .unwrap()
        .pre_condition_rules[0];
    assert_eq!(rule.action.as_str(), "skip");
    assert_eq!(rule.conditions[0].operator.as_str(), "not");
}

#[test]
fn the_package_refusals_apply_to_scorm2004_with_its_edition_attached() {
    let missing = manifest("2004 3rd Edition", PLAIN, "", RESOURCES);
    let refused = import_package(
        Cursor::new(zip(&[("imsmanifest.xml", &missing), ("intro.html", "")])),
        &Limits::default(),
    )
    .unwrap_err();
    assert_eq!(refused.code, RefusalCode::LaunchResourceMissing);
    assert_eq!(refused.standard, Some(Standard::Scorm2004));
    assert_eq!(refused.edition, Some(Edition::Third));

    let remote = RESOURCES.replace(
        r#"href="body.html""#,
        r#"href="https://example.org/body.html""#,
    );
    let refused = refusal(&manifest("2004 3rd Edition", PLAIN, "", &remote));
    assert_eq!(refused.code, RefusalCode::LaunchResourceRemote);
    assert_eq!(refused.edition, Some(Edition::Third));

    // Declared sizes are checked before the manifest is read, so this one
    // comes before the standard is known.
    let plain = manifest("2004 4th Edition", PLAIN, "", RESOURCES);
    let mut files = vec![("imsmanifest.xml", plain.as_str())];
    let big = "x".repeat(4096);
    files.extend(FILES.iter().map(|name| (*name, big.as_str())));
    let small = Limits {
        max_uncompressed_bytes: 8 * 1024,
        ..Limits::default()
    };
    let refused = import_package(Cursor::new(zip(&files)), &small).unwrap_err();
    assert_eq!(refused.code, RefusalCode::LimitExceeded);
}
