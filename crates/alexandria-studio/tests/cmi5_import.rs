//! cmi5 package import: a zip holding `cmi5.xml` and the assignable units it
//! launches becomes the same package description SCORM packages produce.
//!
//! Course structures marked "after" a published one keep that structure's
//! nesting, identifiers, attributes and addresses; long descriptions are left
//! out. The published ones are the CATAPULT LMS test suite and course
//! examples (adlnet/CATAPULT, `lts/pkg/src/`, `course_examples/`) and the
//! examples in section 15 of the cmi5 specification. Every archive is built
//! here.

use std::io::Cursor;

use alexandria_studio::package::{
    import_package, LaunchKind, LaunchMethod, Limits, MoveOn, PackageDescription, Refusal,
    RefusalCode, Standard,
};

#[path = "support/zip_fixture.rs"]
mod fixture;

use fixture::{file, zip, Entry};

const CMI5_NS: &str = "https://w3id.org/xapi/profiles/cmi5/v1/CourseStructure.xsd";

fn import(entries: &[Entry]) -> Result<PackageDescription, Refusal> {
    import_package(Cursor::new(zip(entries)), &Limits::default())
}

fn refusal(entries: &[Entry]) -> Refusal {
    import(entries).expect_err("the package should be refused")
}

/// A one-unit package around a course structure, with `index.html` beside it.
fn package(structure: &str) -> Vec<Entry> {
    vec![
        file("cmi5.xml", structure),
        file("index.html", "<html></html>"),
    ]
}

/// After CATAPULT `001-essentials`: a block holding one unit that sets every
/// attribute and element an assignable unit can carry.
fn essentials() -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<courseStructure xmlns="{CMI5_NS}">
    <course id="https://w3id.org/xapi/cmi5/catapult/lts/course/001-essentials">
        <title><langstring lang="en">CATAPULT LMS Test Course: 001 Essentials</langstring></title>
        <description><langstring lang="en">CATAPULT LMS Test Course: 001 Essentials</langstring></description>
    </course>
    <block id="https://w3id.org/xapi/cmi5/catapult/lts/block/001-essentials">
        <title><langstring lang="en">CATAPULT LMS Test Block: 001 Essentials</langstring></title>
        <description><langstring lang="en">CATAPULT LMS Test Block: 001 Essentials</langstring></description>
        <au
            id="https://w3id.org/xapi/cmi5/catapult/lts/au/001-essentials"
            moveOn="CompletedAndPassed"
            masteryScore="0.9"
            launchMethod="AnyWindow"
        >
            <title><langstring lang="en">CATAPULT LMS Test AU: 001 Essentials</langstring></title>
            <description><langstring lang="en">CATAPULT LMS Test AU: 001 Essentials</langstring></description>
            <url>
                <![CDATA[
                    index.html?paramA=1&paramB=2
                ]]>
            </url>
            <launchParameters>
                sample string
            </launchParameters>
            <entitlementKey>
                sample value
            </entitlementKey>
        </au>
    </block>
</courseStructure>"#
    )
}

#[test]
fn a_unit_in_a_block_imports_with_everything_the_course_structure_says_about_it() {
    let package = import(&package(&essentials())).unwrap();

    assert_eq!(package.standard, Standard::Cmi5);
    assert_eq!(package.edition, None);
    assert_eq!(
        package.identifier,
        "https://w3id.org/xapi/cmi5/catapult/lts/course/001-essentials"
    );
    assert_eq!(package.title, "CATAPULT LMS Test Course: 001 Essentials");

    // The root is the course: never launchable.
    assert_eq!(package.tree.id, package.identifier);
    assert_eq!(package.tree.title, package.title);
    assert!(package.tree.launch.is_none());
    assert_eq!(package.tree.children.len(), 1);

    let block = &package.tree.children[0];
    assert_eq!(
        block.id,
        "https://w3id.org/xapi/cmi5/catapult/lts/block/001-essentials"
    );
    assert_eq!(block.title, "CATAPULT LMS Test Block: 001 Essentials");
    assert!(block.visible);
    assert!(block.launch.is_none());
    assert_eq!(block.children.len(), 1);

    let unit = &block.children[0];
    assert_eq!(
        unit.id,
        "https://w3id.org/xapi/cmi5/catapult/lts/au/001-essentials"
    );
    assert_eq!(unit.title, "CATAPULT LMS Test AU: 001 Essentials");
    assert!(unit.children.is_empty());

    let launch = unit.launch.as_ref().expect("a unit is launchable");
    assert_eq!(launch.kind, LaunchKind::AssignableUnit);
    // Leading and trailing white space is removed from every value (cmi5
    // section 13.1), CDATA or not.
    assert_eq!(launch.path, "index.html");
    assert_eq!(launch.parameters.as_deref(), Some("?paramA=1&paramB=2"));
    assert_eq!(launch.launch_data.as_deref(), Some("sample string"));
    assert_eq!(launch.mastery_score, Some(0.9));

    let unit_cmi5 = unit.cmi5.as_ref().expect("a unit carries its cmi5 details");
    assert_eq!(unit_cmi5.move_on, Some(MoveOn::CompletedAndPassed));
    assert_eq!(unit_cmi5.launch_method, Some(LaunchMethod::AnyWindow));
    assert_eq!(unit_cmi5.entitlement_key.as_deref(), Some("sample value"));
    assert_eq!(unit_cmi5.activity_type, None);
    assert!(unit_cmi5.objectives.is_empty());

    assert_eq!(package.launchable_activities(), vec![unit]);
}

/// After CATAPULT `002-allowed`: a unit straight under the course, stating
/// only `moveOn`.
fn allowed(au_attributes: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<courseStructure xmlns="{CMI5_NS}">
    <course id="https://w3id.org/xapi/cmi5/catapult/lts/course/002-allowed">
        <title><langstring lang="en">CATAPULT LMS Test Course: 002 Allowed</langstring></title>
        <description><langstring lang="en">CATAPULT LMS Test Course: 002 Allowed</langstring></description>
    </course>
    <au id="https://w3id.org/xapi/cmi5/catapult/lts/au/002-allowed" {au_attributes}>
        <title><langstring lang="en">CATAPULT LMS Test AU: 002 Allowed</langstring></title>
        <description><langstring lang="en">CATAPULT LMS Test AU: 002 Allowed</langstring></description>
        <url>index.html</url>
    </au>
</courseStructure>"#
    )
}

#[test]
fn a_unit_that_states_nothing_gets_the_standards_defaults() {
    let package = import(&package(&allowed(""))).unwrap();
    let unit = &package.tree.children[0];
    let launch = unit.launch.as_ref().unwrap();
    assert_eq!(launch.path, "index.html");
    assert_eq!(launch.parameters, None);
    assert_eq!(launch.launch_data, None);
    assert_eq!(launch.mastery_score, None);
    let unit_cmi5 = unit.cmi5.as_ref().unwrap();
    assert_eq!(unit_cmi5.move_on, Some(MoveOn::NotApplicable));
    assert_eq!(unit_cmi5.launch_method, Some(LaunchMethod::AnyWindow));
    assert_eq!(unit_cmi5.entitlement_key, None);
}

#[test]
fn launch_method_and_move_on_are_read_as_written() {
    // CATAPULT 003-launchMethod-OwnWindow and the 004 moveOn courses.
    let cases = [
        (
            r#"launchMethod="OwnWindow""#,
            LaunchMethod::OwnWindow,
            MoveOn::NotApplicable,
        ),
        (
            r#"moveOn="Completed""#,
            LaunchMethod::AnyWindow,
            MoveOn::Completed,
        ),
        (
            r#"moveOn="Passed""#,
            LaunchMethod::AnyWindow,
            MoveOn::Passed,
        ),
        (
            r#"moveOn="CompletedOrPassed""#,
            LaunchMethod::AnyWindow,
            MoveOn::CompletedOrPassed,
        ),
        (
            r#"moveOn="CompletedAndPassed""#,
            LaunchMethod::AnyWindow,
            MoveOn::CompletedAndPassed,
        ),
        (
            r#"moveOn="NotApplicable""#,
            LaunchMethod::AnyWindow,
            MoveOn::NotApplicable,
        ),
        (
            r#"launchMethod=" OwnWindow " moveOn=" Passed ""#,
            LaunchMethod::OwnWindow,
            MoveOn::Passed,
        ),
    ];
    for (attributes, launch_method, move_on) in cases {
        let package = import(&package(&allowed(attributes))).unwrap();
        let unit_cmi5 = package.tree.children[0].cmi5.clone().unwrap();
        assert_eq!(unit_cmi5.launch_method, Some(launch_method), "{attributes}");
        assert_eq!(unit_cmi5.move_on, Some(move_on), "{attributes}");
    }
}

/// Rewrites every unit URL in a published course structure to the path part
/// of the address, as if the course had been packaged with its units, and
/// returns the archive that holds them.
fn packaged(structure: &str) -> Vec<Entry> {
    let mut rewritten = String::new();
    let mut paths = Vec::new();
    let mut rest = structure;
    while let Some(start) = rest.find("<url>") {
        let (before, after) = rest.split_at(start + "<url>".len());
        let end = after.find("</url>").unwrap();
        let address = after[..end].trim();
        let path = address
            .split_once("://")
            .map_or(address, |(_, host_and_path)| {
                host_and_path.split_once('/').map_or("", |(_, path)| path)
            });
        rewritten.push_str(before);
        rewritten.push_str(path);
        paths.push(path.split(['?', '#']).next().unwrap().to_owned());
        rest = &after[end..];
    }
    rewritten.push_str(rest);

    paths.sort();
    paths.dedup();
    let mut entries = vec![file("cmi5.xml", rewritten)];
    entries.extend(paths.iter().map(|path| file(path, "<html></html>")));
    entries
}

/// After section 15.2 of the cmi5 specification (`complex-cmi5.xml`):
/// course objectives, blocks three deep with objective references, units
/// with every attribute, and units on other hosts. Descriptions are cut.
fn complex() -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<courseStructure xmlns="{CMI5_NS}">
  <course id="http://courses.example.edu/identifiers/courses/d07e186b">
    <title>
      <langstring lang="en-US">Geology</langstring>
      <langstring lang="de-DE">Geologie</langstring>
    </title>
    <description><langstring lang="en-US">Geology is an earth science.</langstring></description>
  </course>
  <objectives>
    <objective id="http://objectives.example.com/identifiers/geology/basics">
      <title>
        <langstring lang="en-US">Geology - Basic knowledge</langstring>
        <langstring lang="de-DE">Geologie - Grundwissen</langstring>
      </title>
    </objective>
    <objective
        id="http://objectives.example.com/identifiers/geology/material-identification">
      <title><langstring lang="en-US">Identification of common Earth materials.</langstring></title>
    </objective>
    <objective
        id="http://courses.example.edu/objectives/scientific-thinking-and-acting">
      <title><langstring lang="en-US">Scientific Thinking and Acting</langstring></title>
    </objective>
    <objective id="http://objectives.example.com/identifiers/history/history-of-science">
      <title><langstring lang="en-US">History of Science</langstring></title>
    </objective>
  </objectives>

  <block id="http://courses.example.edu/identifiers/courses/d07e186b/blocks/001">
    <title><langstring lang="en-US">Geologic materials</langstring></title>
    <objectives>
      <objective
          idref="http://objectives.example.com/identifiers/geology/basics"/>
      <objective
          idref="http://objectives.example.com/identifiers/geology/material-identification"/>
    </objectives>
    <au id="http://courses.example.edu/identifiers/courses/d07e186b/blocks/001/aus/64f6"
        activityType="http://adlnet.gov/expapi/activities/lesson" launchMethod="AnyWindow"
        moveOn="CompletedOrPassed" masteryScore="1.0">
      <title><langstring lang="en-US">Rock and rock cycle</langstring></title>
      <url>
        http://courses.example.edu/identifiers/courses/d07e186b/blocks/001/aus/64f6/launch
      </url>
      <launchParameters>{{'initialSpeed':3.0,'mode':1}}</launchParameters>
      <entitlementKey>833d0c7c-a3f8-4f9b-a51f-cbd8a9dac9fb</entitlementKey>
    </au>
    <au id="http://courses.example.edu/identifiers/courses/d07e186b/blocks/001/aus/3ee0"
        activityType="http://adlnet.gov/expapi/activities/lesson" launchMethod="AnyWindow"
        moveOn="NotApplicable">
      <title><langstring lang="en-US">Unconsolidated material</langstring></title>
      <url>
        http://courses.example.edu/identifiers/courses/d07e186b/blocks/001/aus/3ee0/launch
      </url>
      <entitlementKey>833d0c7c-a3f8-4f9b-a51f-cbd8a9dac9fb</entitlementKey>
    </au>
  </block>

  <block id="http://courses.example.edu/identifiers/courses/d07e186b/blocks/002">
    <title><langstring lang="en-US">Whole-Earth structure</langstring></title>
    <objectives>
      <objective
          idref="http://objectives.example.com/identifiers/geology/basics"/>
    </objectives>
    <au id="http://example.com/courses/f59c9fc0/au/6f64"
        activityType="http://adlnet.gov/expapi/activities/lesson"
        launchMethod="OwnWindow" moveOn="Passed" masteryScore="0.1">
      <title><langstring lang="en-US">Plate tectonics</langstring></title>
      <objectives>
        <objective
            idref="http://objectives.example.com/identifiers/history/history-of-science"/>
      </objectives>
      <url>http://example.com/courses/f59c9fc0/au/6f64/start</url>
    </au>
    <au id="http://example.com/courses/f59c9fc0/au/6f65"
        activityType="http://adlnet.gov/expapi/activities/lesson"
        launchMethod="OwnWindow" moveOn="CompletedOrPassed" masteryScore="0.3">
      <title><langstring lang="en-US">Structure of the earth</langstring></title>
      <url>http://example.com/courses/f59c9fc0/au/6f65/start</url>
      <launchParameters>
      </launchParameters>
      <entitlementKey></entitlementKey>
    </au>
  </block>

  <block id="http://courses.example.edu/identifiers/courses/d07e186b/blocks/003">
    <title><langstring lang="en-US">Geologic time scale</langstring></title>
    <objectives>
      <objective
          idref="http://objectives.example.com/identifiers/geology/basics"/>
      <objective
          idref="http://courses.example.edu/objectives/scientific-thinking-and-acting"/>
    </objectives>
    <au id="http://example.com/courses/f59c9fc0/au/6f66"
        activityType="http://adlnet.gov/expapi/activities/lesson"
        launchMethod="OwnWindow" moveOn="CompletedAndPassed" masteryScore="0.5">
      <title><langstring lang="en-US">History and nomenclature of the time scale</langstring></title>
      <url>http://example.com/courses/f59c9fc0/au/6f66/start</url>
    </au>
    <block id="http://courses.example.edu/identifiers/courses/d07e186b/blocks/003-001">
      <title><langstring lang="en-US">Current official geologic time scale</langstring></title>
      <block
          id="http://courses.example.edu/identifiers/courses/d07e186b/blocks/003-001-001">
        <title><langstring lang="en-US">Phanerozoic</langstring></title>
        <au id="http://courses.example.edu/identifiers/courses/d07e186b/blocks/003-001/aus/7ec9"
            activityType="http://adlnet.gov/expapi/activities/lesson" launchMethod="AnyWindow"
            moveOn="Completed">
          <title><langstring lang="en-US">Cenozoic</langstring></title>
          <url>
            http://courses.example.edu/identifiers/courses/d07e186b/blocks/003-001/aus/7ec9/launch
          </url>
        </au>
        <au id="http://courses.example.edu/identifiers/courses/d07e186b/blocks/003-001/aus/7eca/"
            activityType="http://adlnet.gov/expapi/activities/lesson" launchMethod="AnyWindow"
            moveOn="Completed">
          <title><langstring lang="en-US">Mesozoic</langstring></title>
          <url>
            http://courses.example.edu/identifiers/courses/d07e186b/blocks/003-001/aus/7eca/launch
          </url>
        </au>
        <au id="http://courses.example.edu/identifiers/courses/d07e186b/blocks/003-001/aus/7ecb/"
            activityType="http://adlnet.gov/expapi/activities/lesson" launchMethod="AnyWindow"
            moveOn="Completed">
          <title><langstring lang="en-US">Paleozoic</langstring></title>
          <url>
            http://courses.example.edu/identifiers/courses/d07e186b/blocks/003-001/aus/7ecb/launch
          </url>
        </au>
      </block>
      <block
          id="http://courses.example.edu/identifiers/courses/d07e186b/blocks/003-001-002">
        <title><langstring lang="en-US">Proterozoic</langstring></title>
        <au id="http://courses.example.edu/identifiers/courses/d07e186b/blocks/003-001/aus/7ecc/"
            activityType="http://adlnet.gov/expapi/activities/lesson" launchMethod="AnyWindow"
            moveOn="NotApplicable">
          <title><langstring lang="en-US">Neoproterozoic</langstring></title>
          <url>
            http://courses.example.edu/identifiers/courses/d07e186b/blocks/003-001/aus/7ecc/launch
          </url>
        </au>
        <au id="http://courses.example.edu/identifiers/courses/d07e186b/blocks/003-001/aus/7ecd/"
            activityType="http://adlnet.gov/expapi/activities/lesson" launchMethod="AnyWindow">
          <title><langstring lang="en-US">Mesoproterozoic</langstring></title>
          <url>
            http://courses.example.edu/identifiers/courses/d07e186b/blocks/003-001/aus/7ecd/launch
          </url>
        </au>
        <au id="http://courses.example.edu/identifiers/courses/d07e186b/blocks/003-001/aus/7ece/"
            activityType="http://adlnet.gov/expapi/activities/lesson" launchMethod="AnyWindow">
          <title><langstring lang="en-US">Paleoproterozoic</langstring></title>
          <url>
            http://courses.example.edu/identifiers/courses/d07e186b/blocks/003-001/aus/7ece/launch
          </url>
        </au>
      </block>
      <au id="http://courses.example.edu/identifiers/courses/d07e186b/blocks/003-001/aus/7ecf/"
          activityType="http://adlnet.gov/expapi/activities/lesson" launchMethod="AnyWindow">
        <title><langstring lang="en-US">Archean</langstring></title>
        <url>
          http://courses.example.edu/identifiers/courses/d07e186b/blocks/003-001/aus/7ecf/launch
        </url>
      </au>
      <au id="http://courses.example.edu/identifiers/courses/d07e186b/blocks/003-001/aus/7ed0/"
          activityType="http://adlnet.gov/expapi/activities/lesson" launchMethod="AnyWindow"
          moveOn="Passed"  masteryScore="0.5">
        <title><langstring lang="en-US">Hadean</langstring></title>
        <url>
          http://courses.example.edu/identifiers/courses/d07e186b/blocks/003-001/aus/7ed0/launch
        </url>
      </au>
    </block>
  </block>
  <au id="http://quiz-server.example.com/1Hu62hL" masteryScore="0.7"
      launchMethod="OwnWindow" moveOn="Passed"
      activityType="http://adlnet.gov/expapi/activities/assessment">
    <title><langstring lang="en-US">Quiz</langstring></title>
    <url>http://quiz-server.example.com/1Hu62hL</url>
    <launchParameters>
      {{'level':3,'count':25,'_callback':'http://courses.example.edu/quizes/'}}
    </launchParameters>
    <entitlementKey>
      w8GFdWktfOvzQUmFlI1YbUWB4yZX9jyEX3atFKmKW1eN6PTXJKh39wtUYBOvVx1eLt78b6joNZ1r0uj5x20zrSRUKu2
    </entitlementKey>
  </au>
</courseStructure>"#
    )
}

/// One line per activity, indented by depth: the shape of a tree at a glance.
fn outline(activity: &alexandria_studio::package::Activity, depth: usize, out: &mut Vec<String>) {
    let kind = match &activity.launch {
        Some(launch) => format!("unit {}", launch.path),
        None => "heading".to_owned(),
    };
    out.push(format!("{}{} ({kind})", "  ".repeat(depth), activity.title));
    for child in &activity.children {
        outline(child, depth + 1, out);
    }
}

#[test]
fn the_specifications_complex_example_imports_as_a_tree_once_its_units_are_packaged() {
    let package = import(&packaged(&complex())).unwrap();

    assert_eq!(
        package.identifier,
        "http://courses.example.edu/identifiers/courses/d07e186b"
    );
    // The first title the author wrote.
    assert_eq!(package.title, "Geology");

    let mut lines = Vec::new();
    outline(&package.tree, 0, &mut lines);
    let au = "identifiers/courses/d07e186b/blocks";
    assert_eq!(
        lines,
        [
            "Geology (heading)".to_owned(),
            "  Geologic materials (heading)".to_owned(),
            format!("    Rock and rock cycle (unit {au}/001/aus/64f6/launch)"),
            format!("    Unconsolidated material (unit {au}/001/aus/3ee0/launch)"),
            "  Whole-Earth structure (heading)".to_owned(),
            "    Plate tectonics (unit courses/f59c9fc0/au/6f64/start)".to_owned(),
            "    Structure of the earth (unit courses/f59c9fc0/au/6f65/start)".to_owned(),
            "  Geologic time scale (heading)".to_owned(),
            "    History and nomenclature of the time scale (unit courses/f59c9fc0/au/6f66/start)"
                .to_owned(),
            "    Current official geologic time scale (heading)".to_owned(),
            "      Phanerozoic (heading)".to_owned(),
            format!("        Cenozoic (unit {au}/003-001/aus/7ec9/launch)"),
            format!("        Mesozoic (unit {au}/003-001/aus/7eca/launch)"),
            format!("        Paleozoic (unit {au}/003-001/aus/7ecb/launch)"),
            "      Proterozoic (heading)".to_owned(),
            format!("        Neoproterozoic (unit {au}/003-001/aus/7ecc/launch)"),
            format!("        Mesoproterozoic (unit {au}/003-001/aus/7ecd/launch)"),
            format!("        Paleoproterozoic (unit {au}/003-001/aus/7ece/launch)"),
            format!("      Archean (unit {au}/003-001/aus/7ecf/launch)"),
            format!("      Hadean (unit {au}/003-001/aus/7ed0/launch)"),
            "  Quiz (unit 1Hu62hL)".to_owned(),
        ]
    );
    assert_eq!(package.launchable_activities().len(), 14);

    // The course's objectives, and the blocks and units that refer to them.
    let course = package.cmi5.as_ref().unwrap();
    let objectives: Vec<(&str, &str)> = course
        .objectives
        .iter()
        .map(|objective| (objective.id.as_str(), objective.title.as_str()))
        .collect();
    assert_eq!(
        objectives,
        [
            (
                "http://objectives.example.com/identifiers/geology/basics",
                "Geology - Basic knowledge"
            ),
            (
                "http://objectives.example.com/identifiers/geology/material-identification",
                "Identification of common Earth materials."
            ),
            (
                "http://courses.example.edu/objectives/scientific-thinking-and-acting",
                "Scientific Thinking and Acting"
            ),
            (
                "http://objectives.example.com/identifiers/history/history-of-science",
                "History of Science"
            ),
        ]
    );
    let materials = &package.tree.children[0];
    assert_eq!(
        materials.cmi5.as_ref().unwrap().objectives,
        [
            "http://objectives.example.com/identifiers/geology/basics",
            "http://objectives.example.com/identifiers/geology/material-identification",
        ]
    );
    assert_eq!(materials.cmi5.as_ref().unwrap().move_on, None);
    let tectonics = &package.tree.children[1].children[0].cmi5.as_ref().unwrap();
    assert_eq!(
        tectonics.objectives,
        ["http://objectives.example.com/identifiers/history/history-of-science"]
    );
    assert_eq!(tectonics.launch_method, Some(LaunchMethod::OwnWindow));
    assert_eq!(tectonics.move_on, Some(MoveOn::Passed));

    // Every attribute of the first unit.
    let rock = &materials.children[0];
    assert_eq!(
        rock.id,
        "http://courses.example.edu/identifiers/courses/d07e186b/blocks/001/aus/64f6"
    );
    let launch = rock.launch.as_ref().unwrap();
    assert_eq!(launch.mastery_score, Some(1.0));
    assert_eq!(
        launch.launch_data.as_deref(),
        Some("{'initialSpeed':3.0,'mode':1}")
    );
    let rock_cmi5 = rock.cmi5.as_ref().unwrap();
    assert_eq!(rock_cmi5.move_on, Some(MoveOn::CompletedOrPassed));
    assert_eq!(rock_cmi5.launch_method, Some(LaunchMethod::AnyWindow));
    assert_eq!(
        rock_cmi5.entitlement_key.as_deref(),
        Some("833d0c7c-a3f8-4f9b-a51f-cbd8a9dac9fb")
    );
    assert_eq!(
        rock_cmi5.activity_type.as_deref(),
        Some("http://adlnet.gov/expapi/activities/lesson")
    );

    // Elements present but empty are absent, not empty strings.
    let structure = &package.tree.children[1].children[1];
    assert_eq!(structure.launch.as_ref().unwrap().launch_data, None);
    assert_eq!(structure.cmi5.as_ref().unwrap().entitlement_key, None);

    // A unit with a trailing slash in its identifier keeps it: the
    // identifier is the key, not a path.
    let mesozoic = &package.tree.children[2].children[1].children[0].children[1];
    assert_eq!(
        mesozoic.id,
        "http://courses.example.edu/identifiers/courses/d07e186b/blocks/003-001/aus/7eca/"
    );

    let quiz = package.tree.children.last().unwrap();
    assert_eq!(quiz.launch.as_ref().unwrap().mastery_score, Some(0.7));
    assert_eq!(
        quiz.launch.as_ref().unwrap().launch_data.as_deref(),
        Some("{'level':3,'count':25,'_callback':'http://courses.example.edu/quizes/'}")
    );
}

/// Section 15.1 of the cmi5 specification (`simple-cmi5.xml`), whole.
fn simple() -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<courseStructure xmlns="{CMI5_NS}">
  <course id="http://course-repository.example.edu/identifiers/courses/02baafcf">
    <title>
      <langstring lang="en-US">Introduction to Geology</langstring>
    </title>
    <description>
      <langstring lang="en-US">
        This course will introduce you into the basics of geology. This includes subjects such as
        plate tectonics, geological materials and the history of the Earth.
      </langstring>
    </description>
  </course>
  <au id="http://course-repository.example.edu/identifiers/courses/02baafcf/aus/4c07">
    <title>
      <langstring lang="en-US">Introduction to Geology</langstring>
    </title>
    <description>
      <langstring lang="en-US">
        This course will introduce you into the basics of geology. This includes subjects such as
        plate tectonics, geological materials and the history of the Earth.
      </langstring>
    </description>
    <url>http://course-repository.example.edu/identifiers/courses/02baafcf/aus/4c07/launch.html</url>
  </au>
</courseStructure>"#
    )
}

#[test]
fn the_specifications_simple_example_imports_once_its_unit_is_packaged() {
    let package = import(&packaged(&simple())).unwrap();
    assert_eq!(package.title, "Introduction to Geology");
    let units = package.launchable_activities();
    assert_eq!(units.len(), 1);
    assert_eq!(
        units[0].id,
        "http://course-repository.example.edu/identifiers/courses/02baafcf/aus/4c07"
    );
    assert_eq!(
        units[0].launch.as_ref().unwrap().path,
        "identifiers/courses/02baafcf/aus/4c07/launch.html"
    );
}

#[test]
fn the_specifications_examples_as_published_launch_remote_units_and_are_refused() {
    for structure in [simple(), complex()] {
        let refused = refusal(&package(&structure));
        assert_eq!(refused.code, RefusalCode::LaunchResourceRemote);
        assert_eq!(refused.standard, Some(Standard::Cmi5));
        assert!(refused.detail.contains("http://"), "{}", refused.detail);
    }
}

#[test]
fn one_remote_unit_among_packaged_ones_refuses_the_package() {
    // cmi5 allows the mix (section 14.1); the learning standards spec does not.
    let mut entries = packaged(&complex());
    let structure = complex().replacen(
        "http://example.com/courses/f59c9fc0/au/6f64/start",
        "courses/f59c9fc0/au/6f64/start",
        1,
    );
    entries[0] = file("cmi5.xml", structure);
    let refused = refusal(&entries);
    assert_eq!(refused.code, RefusalCode::LaunchResourceRemote);

    let remote = [
        "https://cdn.example.org/au/index.html",
        "//cdn.example.org/au/index.html",
        "file:///etc/passwd",
        "javascript:alert(1)",
        // "an application specific protocol MAY be used" (section 13.1.4).
        "myapp://course/au/1",
    ];
    for address in remote {
        let structure =
            allowed("").replace("<url>index.html</url>", &format!("<url>{address}</url>"));
        let refused = refusal(&package(&structure));
        assert_eq!(refused.code, RefusalCode::LaunchResourceRemote, "{address}");
        assert!(
            refused
                .detail
                .contains("https://w3id.org/xapi/cmi5/catapult/lts/au/002-allowed"),
            "the refusal names the unit: {}",
            refused.detail
        );
    }
}

/// After CATAPULT `course_examples/multi_au_framed`: eight units launching
/// one file with different queries.
#[test]
fn units_sharing_a_file_keep_their_own_query() {
    let mut units = String::new();
    for page in ["1", "2", "3", "quiz"] {
        units.push_str(&format!(
            r#"<au id="https://w3id.org/xapi/cmi5/catapult/lts/course/geology-intro-multi-au-framed/{page}"
                   moveOn="CompletedOrPassed">
                 <title><langstring lang="en-US">Page {page}</langstring></title>
                 <url>index.html?pages={page}&amp;complete=launch</url>
               </au>"#
        ));
    }
    let structure = format!(
        r#"<courseStructure xmlns="{CMI5_NS}">
             <course id="https://w3id.org/xapi/cmi5/catapult/lts/course/geology-intro-multi-au-framed">
               <title><langstring lang="en-US">Introduction to Geology</langstring></title>
             </course>
             {units}
           </courseStructure>"#
    );
    let package = import(&package(&structure)).unwrap();
    let parameters: Vec<_> = package
        .launchable_activities()
        .iter()
        .map(|unit| {
            let launch = unit.launch.as_ref().unwrap();
            assert_eq!(launch.path, "index.html");
            launch.parameters.clone().unwrap()
        })
        .collect();
    assert_eq!(
        parameters,
        [
            "?pages=1&complete=launch",
            "?pages=2&complete=launch",
            "?pages=3&complete=launch",
            "?pages=quiz&complete=launch",
        ]
    );
}

// ---- refusals: the CATAPULT LMS test suite's import failures --------------

#[test]
fn an_identifier_that_is_not_an_absolute_iri_is_refused() {
    // CATAPULT 201-1 to 201-4: course, block, unit and objective identifiers
    // with the scheme left off (cmi5 requirement 3.0.0.0-1).
    let course = allowed("").replace(
        r#"<course id="https://w3id.org/"#,
        r#"<course id="w3id.org/"#,
    );
    let unit = allowed("").replace(r#"<au id="https://w3id.org/"#, r#"<au id="w3id.org/"#);
    let block = essentials().replace(r#"<block id="https://w3id.org/"#, r#"<block id="w3id.org/"#);
    let objective = allowed("").replace(
        "    <au ",
        r#"    <objectives>
        <objective id="w3id.org/xapi/cmi5/catapult/lts/objective/201-4-iris-objective-id">
            <title><langstring lang="en">Objective</langstring></title>
        </objective>
    </objectives>
    <au "#,
    );
    let spaced = allowed("").replace(
        r#"<au id="https://w3id.org/xapi/cmi5/catapult/lts/au/002-allowed""#,
        r#"<au id="https://w3id.org/xapi/cmi5/catapult/lts/au/002 allowed""#,
    );
    for (what, structure) in [
        ("course", course),
        ("unit", unit),
        ("block", block),
        ("objective", objective),
        ("white space", spaced),
    ] {
        let refused = refusal(&package(&structure));
        assert_eq!(refused.code, RefusalCode::ManifestUnparseable, "{what}");
        assert_eq!(refused.standard, Some(Standard::Cmi5), "{what}");
    }
}

#[test]
fn an_identifier_used_twice_is_refused() {
    // CATAPULT 205-1 to 205-3 (cmi5 13.1.2, 13.1.3, 13.1.4), units packaged.
    let block = essentials().replace(
        "</courseStructure>",
        r#"<block id="https://w3id.org/xapi/cmi5/catapult/lts/block/001-essentials">
             <title><langstring lang="en">Again</langstring></title>
             <au id="https://w3id.org/xapi/cmi5/catapult/lts/au/001-essentials/1"><url>index.html</url></au>
           </block>
           </courseStructure>"#,
    );
    let unit = allowed("").replace(
        "</courseStructure>",
        r#"<au id="https://w3id.org/xapi/cmi5/catapult/lts/au/002-allowed"><url>index.html</url></au>
           </courseStructure>"#,
    );
    let objective_xml = r#"<objective id="http://w3id.org/xapi/cmi5/catapult/lts/objective/205-2">
                             <title><langstring lang="en">O</langstring></title>
                           </objective>"#;
    let objective = allowed("").replace(
        "    <au ",
        &format!("<objectives>{objective_xml}{objective_xml}</objectives>\n    <au "),
    );
    for (what, structure) in [("block", block), ("unit", unit), ("objective", objective)] {
        let refused = refusal(&package(&structure));
        assert_eq!(refused.code, RefusalCode::ManifestUnparseable, "{what}");
        assert!(
            refused.detail.contains("more than once"),
            "{}",
            refused.detail
        );
    }
}

#[test]
fn a_unit_url_whose_query_uses_a_cmi5_launch_parameter_name_is_refused() {
    // CATAPULT 204: the launch would carry two of the same parameter
    // (cmi5 section 8.1).
    for name in ["endpoint", "fetch", "actor", "registration", "activityId"] {
        let structure = allowed("").replace(
            "<url>index.html</url>",
            &format!("<url>index.html?mode=1&amp;{name}=http://example.org/lrs</url>"),
        );
        let refused = refusal(&package(&structure));
        assert_eq!(refused.code, RefusalCode::ManifestUnparseable, "{name}");
        assert!(refused.detail.contains(name), "{}", refused.detail);
    }
    // Names that only contain or resemble one are the unit's own business.
    let structure = allowed("").replace(
        "<url>index.html</url>",
        "<url>index.html?endpoints=1&amp;Actor=2&amp;x=endpoint#fetch=3</url>",
    );
    import(&package(&structure)).unwrap();
}

#[test]
fn a_unit_whose_file_is_not_in_the_archive_is_refused_as_missing() {
    // CATAPULT 203-1 (cmi5 14.1.0.0-4).
    let structure = allowed("").replace("<url>index.html</url>", "<url>not-found.html</url>");
    let refused = refusal(&package(&structure));
    assert_eq!(refused.code, RefusalCode::LaunchResourceMissing);
    assert_eq!(refused.standard, Some(Standard::Cmi5));
    assert!(
        refused.detail.contains("not-found.html"),
        "{}",
        refused.detail
    );

    for url in ["<url>../index.html</url>", "<url>  </url>", ""] {
        let structure = allowed("").replace("<url>index.html</url>", url);
        let refused = refusal(&package(&structure));
        assert_eq!(refused.code, RefusalCode::LaunchResourceMissing, "{url:?}");
    }
}

#[test]
fn an_address_rooted_at_the_package_resolves_inside_it() {
    // CATAPULT 202-2 and 202-5 use these as standalone course structures,
    // where they are wrong; inside a package they name its files.
    for (url, path) in [
        ("/index.html", "index.html"),
        ("path/1/index.html?abc=def", "path/1/index.html"),
    ] {
        let structure = allowed("").replace("<url>index.html</url>", &format!("<url>{url}</url>"));
        let package = import(&[
            file("cmi5.xml", structure),
            file("index.html", ""),
            file("path/1/index.html", ""),
        ])
        .unwrap();
        assert_eq!(package.tree.children[0].launch.as_ref().unwrap().path, path);
    }
}

#[test]
fn a_course_structure_without_an_archive_around_it_is_refused() {
    // CATAPULT 202 and 208: cmi5 allows a standalone cmi5.xml, but it may
    // only name remote units, which are refused anyway.
    let refused =
        import_package(Cursor::new(allowed("")), &Limits::default()).expect_err("not an archive");
    assert_eq!(refused.code, RefusalCode::NotAnArchive);
}

#[test]
fn an_archive_with_the_course_structure_one_folder_down_is_refused() {
    // CATAPULT 210-1 (cmi5 14.1.0.0-2).
    let refused = refusal(&[
        file("course/cmi5.xml", allowed("")),
        file("course/index.html", ""),
    ]);
    assert_eq!(refused.code, RefusalCode::NoManifest);
    assert!(
        refused.detail.contains("course/cmi5.xml"),
        "{}",
        refused.detail
    );
}

#[test]
fn values_outside_what_the_course_structure_schema_allows_are_refused() {
    let invalid = [
        r#"moveOn="Sometimes""#,
        r#"launchMethod="NewTab""#,
        r#"masteryScore="1.5""#,
        r#"masteryScore="-0.1""#,
        r#"masteryScore="ninety""#,
        r#"masteryScore="NaN""#,
        r#"masteryScore="inf""#,
        r#"masteryScore="1e-1""#,
        r#"masteryScore="0,5""#,
    ];
    for attributes in invalid {
        let refused = refusal(&package(&allowed(attributes)));
        assert_eq!(
            refused.code,
            RefusalCode::ManifestUnparseable,
            "{attributes}"
        );
    }
    for (attributes, score) in [
        (r#"masteryScore="0""#, 0.0),
        (r#"masteryScore="1""#, 1.0),
        (r#"masteryScore=" .75 ""#, 0.75),
        (r#"masteryScore="+0.8500""#, 0.85),
    ] {
        let package = import(&package(&allowed(attributes))).unwrap();
        let launch = package.tree.children[0].launch.clone().unwrap();
        assert_eq!(launch.mastery_score, Some(score), "{attributes}");
    }
}

#[test]
fn a_course_structure_missing_what_the_standard_requires_is_refused() {
    let no_course = format!(
        r#"<courseStructure xmlns="{CMI5_NS}"><au id="https://example.org/au"><url>index.html</url></au></courseStructure>"#
    );
    let course_without_id = allowed("").replace(
        r#"<course id="https://w3id.org/xapi/cmi5/catapult/lts/course/002-allowed">"#,
        "<course>",
    );
    let unit_without_id = allowed("").replace(
        r#"<au id="https://w3id.org/xapi/cmi5/catapult/lts/au/002-allowed" >"#,
        "<au>",
    );
    let wrong_root = allowed("").replace("courseStructure", "course-structure");
    let malformed = allowed("").replace("</au>", "");
    for (what, structure) in [
        ("no course", no_course),
        ("course without id", course_without_id),
        ("unit without id", unit_without_id),
        ("wrong root", wrong_root),
        ("malformed", malformed),
    ] {
        let refused = refusal(&package(&structure));
        assert_eq!(refused.code, RefusalCode::ManifestUnparseable, "{what}");
        assert_eq!(refused.standard, Some(Standard::Cmi5), "{what}");
    }
}

#[test]
fn a_course_structure_with_no_unit_is_refused_as_missing() {
    let structure = format!(
        r#"<courseStructure xmlns="{CMI5_NS}">
             <course id="https://example.org/course"><title><langstring>C</langstring></title></course>
             <block id="https://example.org/block"><title><langstring>B</langstring></title></block>
           </courseStructure>"#
    );
    let refused = refusal(&package(&structure));
    assert_eq!(refused.code, RefusalCode::LaunchResourceMissing);
}

// ---- what else import does with a cmi5 package ----------------------------

#[test]
fn vendor_extensions_are_ignored() {
    // After `extended-cmi5.xml` in the cmi5 specification (section 13.1.5),
    // plus an extension element that happens to be called `au`.
    let structure = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<courseStructure xmlns="{CMI5_NS}"
                 xmlns:kw="http://www.adlnet.gov/cmi5/KeywordExtension.xsd">
  <course id="http://course-repository.example.edu/identifiers/courses/02baafcf">
    <title><langstring lang="en-US">Introduction to Geology</langstring></title>
  </course>
  <au id="http://course-repository.example.edu/identifiers/courses/02baafcf/aus/4c07"
      kw:level="2">
    <title><langstring lang="en-US">Introduction to Geology</langstring></title>
    <url>index.html</url>
    <kw:keyword idref="http://words.example/earth_science"></kw:keyword>
    <kw:keyword idref="http://words.example/plate_tectonics"></kw:keyword>
  </au>
  <kw:au id="http://words.example/not-a-unit"><kw:url>https://words.example/</kw:url></kw:au>
  <kw:keywords>
    <kw:keyword id="http://words.example/earth_science">
      <kw:title><langstring lang="en-US">Earth Science</langstring></kw:title>
    </kw:keyword>
  </kw:keywords>
</courseStructure>"#
    );
    let package = import(&package(&structure)).unwrap();
    assert_eq!(package.tree.children.len(), 1);
    assert_eq!(package.launchable_activities().len(), 1);
}

#[test]
fn a_scorm_manifest_beside_a_course_structure_decides() {
    let manifest = r#"<?xml version="1.0"?>
<manifest identifier="com.example.course" xmlns:adlcp="http://www.adlnet.org/xsd/adlcp_rootv1p2">
  <organizations default="org"><organization identifier="org"><title>T</title>
    <item identifier="i" identifierref="r"><title>L</title></item>
  </organization></organizations>
  <resources><resource identifier="r" type="webcontent" adlcp:scormtype="sco" href="index.html"/></resources>
</manifest>"#;
    let package = import(&[
        file("imsmanifest.xml", manifest),
        file("cmi5.xml", allowed("")),
        file("index.html", ""),
    ])
    .unwrap();
    assert_eq!(package.standard, Standard::Scorm12);
    assert_eq!(package.cmi5, None);
    assert_eq!(package.tree.children[0].cmi5, None);
}

#[test]
fn the_archive_limits_apply_to_a_cmi5_package() {
    let tight = Limits {
        max_uncompressed_bytes: 1_000,
        ..Limits::default()
    };
    let mut entries = package(&allowed(""));
    entries.push(file("media/video.bin", vec![7u8; 4_000]));
    let refused = import_package(Cursor::new(zip(&entries)), &tight).expect_err("too big");
    assert_eq!(refused.code, RefusalCode::LimitExceeded);

    // Declared sizes are checked before the course structure is read; what
    // the measuring pass after it finds is reported as a cmi5 refusal.
    let mut entries = package(&allowed(""));
    entries.push(
        file("media/blank.bin", vec![0u8; 8 << 20])
            .deflated()
            .declaring_size(12),
    );
    let refused = refusal(&entries);
    assert_eq!(refused.code, RefusalCode::NotAnArchive);
    assert_eq!(refused.standard, Some(Standard::Cmi5));

    let mut entries = package(&allowed(""));
    entries.push(file("../escape.html", ""));
    assert_eq!(refusal(&entries).code, RefusalCode::UnsafeEntryPath);
}

#[test]
fn a_cmi5_description_is_stored_as_this_json_and_read_back() {
    let package = import(&package(&essentials().replace(
        r#"launchMethod="AnyWindow""#,
        r#"launchMethod="OwnWindow" activityType="http://adlnet.gov/expapi/activities/lesson""#,
    ).replace(
        "    <block ",
        r#"    <objectives>
        <objective id="https://example.org/objectives/lifting">
            <title><langstring lang="en">Lifting</langstring></title>
        </objective>
    </objectives>
    <block "#,
    ).replace(
        "        <au\n",
        r#"        <objectives><objective idref="https://example.org/objectives/lifting"/></objectives>
        <au
"#,
    )))
    .unwrap();

    let json = serde_json::to_value(&package).unwrap();
    let au = "https://w3id.org/xapi/cmi5/catapult/lts/au/001-essentials";
    let expected = serde_json::json!({
        "standard": "cmi5",
        "identifier": "https://w3id.org/xapi/cmi5/catapult/lts/course/001-essentials",
        "title": "CATAPULT LMS Test Course: 001 Essentials",
        "files": 2,
        "uncompressed_bytes": package.uncompressed_bytes,
        "cmi5": { "objectives": [
            { "id": "https://example.org/objectives/lifting", "title": "Lifting" }
        ] },
        "tree": {
            "id": "https://w3id.org/xapi/cmi5/catapult/lts/course/001-essentials",
            "title": "CATAPULT LMS Test Course: 001 Essentials",
            "visible": true,
            "children": [{
                "id": "https://w3id.org/xapi/cmi5/catapult/lts/block/001-essentials",
                "title": "CATAPULT LMS Test Block: 001 Essentials",
                "visible": true,
                "cmi5": { "objectives": ["https://example.org/objectives/lifting"] },
                "children": [{
                    "id": au,
                    "title": "CATAPULT LMS Test AU: 001 Essentials",
                    "visible": true,
                    "launch": {
                        "path": "index.html",
                        "parameters": "?paramA=1&paramB=2",
                        "kind": "assignable_unit",
                        "launch_data": "sample string",
                        "mastery_score": 0.9
                    },
                    "cmi5": {
                        "move_on": "completed_and_passed",
                        "launch_method": "own_window",
                        "entitlement_key": "sample value",
                        "activity_type": "http://adlnet.gov/expapi/activities/lesson"
                    }
                }]
            }]
        }
    });
    assert_eq!(json, expected);

    let back: PackageDescription = serde_json::from_value(json).unwrap();
    assert_eq!(back, package);
    assert_eq!(MoveOn::CompletedAndPassed.as_cmi5(), "CompletedAndPassed");
}
