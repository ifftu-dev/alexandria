//! cmi5 course structures (`cmi5.xml`): the course becomes the root of the
//! activity tree, each block a heading over its members, each assignable
//! unit a launchable activity.
//!
//! Only units packaged in the archive are accepted. The cmi5 specification
//! allows a unit to live on a server and be named by a fully qualified URL
//! (section 14.1); the learning standards spec rules those out, so one such
//! unit refuses the whole package as [`RefusalCode::LaunchResourceRemote`].

use std::collections::HashSet;
use std::io::{Read, Seek};

use percent_encoding::percent_decode_str;
use serde::{Deserialize, Serialize};

use super::archive::Archive;
use super::scorm::launch_target;
use super::xml::Element;
use super::{Activity, Launch, LaunchKind, ParsedManifest, Refusal, RefusalCode};

/// What cmi5 says about the course as a whole, beyond its tree.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Cmi5Course {
    /// The objectives the course structure defines. Blocks and units refer
    /// to them by identifier ([`Cmi5Activity::objectives`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub objectives: Vec<Cmi5Objective>,
}

/// One learning objective a course structure defines.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Cmi5Objective {
    /// The objective's IRI, unique in the course structure.
    pub id: String,
    pub title: String,
}

/// What cmi5 says about one block or assignable unit beyond its place in
/// the tree and its [`Launch`].
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Cmi5Activity {
    /// When the unit counts as satisfied. Set on every unit, with the
    /// standard's default filled in; never set on a block.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub move_on: Option<MoveOn>,
    /// Whether the unit needs a window of its own. Set on every unit, with
    /// the standard's default filled in; never set on a block.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub launch_method: Option<LaunchMethod>,
    /// The unit's `entitlementKey`, handed to it in the launch data document.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entitlement_key: Option<String>,
    /// The unit's `activityType` IRI, for showing what kind of thing it is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activity_type: Option<String>,
    /// Identifiers of the course objectives this block or unit refers to,
    /// as written.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub objectives: Vec<String>,
}

/// The cmi5 `moveOn` criterion: which verbs satisfy a unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MoveOn {
    Passed,
    Completed,
    CompletedAndPassed,
    CompletedOrPassed,
    /// Satisfied without any statement. The default.
    NotApplicable,
}

impl MoveOn {
    /// The value as cmi5 spells it, which is what the launch data document
    /// carries.
    pub fn as_cmi5(self) -> &'static str {
        match self {
            MoveOn::Passed => "Passed",
            MoveOn::Completed => "Completed",
            MoveOn::CompletedAndPassed => "CompletedAndPassed",
            MoveOn::CompletedOrPassed => "CompletedOrPassed",
            MoveOn::NotApplicable => "NotApplicable",
        }
    }

    const ALL: [MoveOn; 5] = [
        MoveOn::Passed,
        MoveOn::Completed,
        MoveOn::CompletedAndPassed,
        MoveOn::CompletedOrPassed,
        MoveOn::NotApplicable,
    ];
}

/// The cmi5 `launchMethod`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaunchMethod {
    /// The player chooses: a frame is fine. The default.
    AnyWindow,
    /// The unit must have a window of its own, or the whole current one.
    OwnWindow,
}

/// A course structure's parts, for [`super::import_package`] to finish.
pub(crate) struct ParsedCourse {
    pub manifest: ParsedManifest,
    pub course: Cmi5Course,
}

pub(crate) fn parse<R: Read + Seek>(
    structure: &Element,
    archive: &Archive<R>,
) -> Result<ParsedCourse, Refusal> {
    if structure.name != "courseStructure" {
        return Err(invalid(format!(
            "the course structure's root element is <{}>, not <courseStructure>",
            structure.name
        )));
    }
    let ns = structure.ns.as_deref();
    let mut context = Context {
        archive,
        ns,
        seen: HashSet::new(),
    };
    let course = own(structure, "course", ns)
        .next()
        .ok_or_else(|| invalid("the course structure has no <course>"))?;
    let identifier = context.identifier(course, "course")?;
    let title = context.title(course).unwrap_or_else(|| identifier.clone());

    let objectives = own(structure, "objectives", ns)
        .flat_map(|objectives| own(objectives, "objective", ns))
        .map(|objective| {
            let id = context.identifier(objective, "objective")?;
            Ok(Cmi5Objective {
                title: context.title(objective).unwrap_or_else(|| id.clone()),
                id,
            })
        })
        .collect::<Result<Vec<_>, Refusal>>()?;

    let children = context.members(structure)?;
    let tree = Activity {
        id: identifier.clone(),
        title: title.clone(),
        visible: true,
        launch: None,
        prerequisites: None,
        sequencing: None,
        completion_threshold: None,
        hide_lms_ui: Vec::new(),
        shared_data: Vec::new(),
        children,
        cmi5: None,
    };
    if !has_launch(&tree) {
        return Err(Refusal::new(
            RefusalCode::LaunchResourceMissing,
            "the course structure has no assignable unit",
        ));
    }
    Ok(ParsedCourse {
        manifest: ParsedManifest {
            identifier,
            title,
            tree,
        },
        course: Cmi5Course { objectives },
    })
}

struct Context<'a, R> {
    archive: &'a Archive<R>,
    /// The course structure's own namespace. Elements in any other are
    /// vendor extensions (cmi5 section 13.1.5), which an importer may ignore
    /// and this one does.
    ns: Option<&'a str>,
    /// Identifiers already used: every block, unit and objective identifier
    /// must be unique in the course structure.
    seen: HashSet<String>,
}

/// Children with this name in the course structure's own namespace.
fn own<'e>(
    parent: &'e Element,
    name: &'e str,
    ns: Option<&'e str>,
) -> impl Iterator<Item = &'e Element> {
    parent
        .children_named(name)
        .filter(move |child| child.ns.as_deref() == ns)
}

impl<R: Read + Seek> Context<'_, R> {
    /// The blocks and units directly under `parent`, in document order.
    fn members(&mut self, parent: &Element) -> Result<Vec<Activity>, Refusal> {
        let mut members = Vec::new();
        for child in &parent.children {
            if child.ns.as_deref() != self.ns {
                continue;
            }
            match child.name.as_str() {
                "block" => members.push(self.block(child)?),
                "au" => members.push(self.unit(child)?),
                _ => {}
            }
        }
        Ok(members)
    }

    fn block(&mut self, block: &Element) -> Result<Activity, Refusal> {
        let id = self.identifier(block, "block")?;
        let objectives = self.objective_references(block);
        let children = self.members(block)?;
        Ok(Activity {
            title: self.title(block).unwrap_or_else(|| id.clone()),
            visible: true,
            launch: None,
            prerequisites: None,
            sequencing: None,
            completion_threshold: None,
            hide_lms_ui: Vec::new(),
            shared_data: Vec::new(),
            children,
            cmi5: Some(Cmi5Activity {
                objectives,
                ..Cmi5Activity::default()
            }),
            id,
        })
    }

    fn unit(&mut self, unit: &Element) -> Result<Activity, Refusal> {
        let id = self.identifier(unit, "au")?;
        let address = self.text(unit, "url").ok_or_else(|| {
            Refusal::new(
                RefusalCode::LaunchResourceMissing,
                format!("unit {id} names no url"),
            )
        })?;
        let target = launch_target(&address, self.archive).map_err(|mut refusal| {
            refusal.detail = format!("unit {id}: {}", refusal.detail);
            refusal
        })?;
        if let Some(name) = launch_parameter_clash(&target.suffix) {
            return Err(invalid(format!(
                "unit {id} has a url whose query already uses {name}, \
                 which the launch adds"
            )));
        }

        let mastery_score = match attribute(unit, "masteryScore") {
            None => None,
            Some(text) => Some(mastery_score(text).ok_or_else(|| {
                invalid(format!(
                    "unit {id} has mastery score {text:?}; it must be a decimal from 0 to 1"
                ))
            })?),
        };
        let move_on = match attribute(unit, "moveOn") {
            None => MoveOn::NotApplicable,
            Some(text) => MoveOn::ALL
                .into_iter()
                .find(|move_on| move_on.as_cmi5().eq_ignore_ascii_case(text))
                .ok_or_else(|| invalid(format!("unit {id} has moveOn {text:?}")))?,
        };
        let launch_method = match attribute(unit, "launchMethod") {
            None => LaunchMethod::AnyWindow,
            Some(text) if text.eq_ignore_ascii_case("AnyWindow") => LaunchMethod::AnyWindow,
            Some(text) if text.eq_ignore_ascii_case("OwnWindow") => LaunchMethod::OwnWindow,
            Some(text) => {
                return Err(invalid(format!(
                    "unit {id} has launchMethod {text:?}; it must be AnyWindow or OwnWindow"
                )))
            }
        };

        Ok(Activity {
            title: self.title(unit).unwrap_or_else(|| id.clone()),
            visible: true,
            launch: Some(Launch {
                path: target.path,
                parameters: Some(target.suffix).filter(|suffix| !suffix.is_empty()),
                kind: LaunchKind::AssignableUnit,
                launch_data: self.text(unit, "launchParameters"),
                mastery_score,
                max_time_allowed: None,
                time_limit_action: None,
            }),
            prerequisites: None,
            sequencing: None,
            completion_threshold: None,
            hide_lms_ui: Vec::new(),
            shared_data: Vec::new(),
            children: Vec::new(),
            cmi5: Some(Cmi5Activity {
                move_on: Some(move_on),
                launch_method: Some(launch_method),
                entitlement_key: self.text(unit, "entitlementKey"),
                activity_type: attribute(unit, "activityType").map(str::to_owned),
                objectives: self.objective_references(unit),
            }),
            id,
        })
    }

    /// The `idref` of each `<objective>` under the element's `<objectives>`.
    fn objective_references(&self, element: &Element) -> Vec<String> {
        own(element, "objectives", self.ns)
            .flat_map(|objectives| own(objectives, "objective", self.ns))
            .filter_map(|objective| attribute(objective, "idref").map(str::to_owned))
            .collect()
    }

    /// The trimmed text of a child element, if it has any.
    fn text(&self, element: &Element, name: &str) -> Option<String> {
        own(element, name, self.ns)
            .next()
            .map(|child| child.text.trim())
            .filter(|text| !text.is_empty())
            .map(str::to_owned)
    }

    /// The element's title on one line: its first non-empty `<langstring>`.
    /// cmi5 titles come in as many languages as the author wrote; the first
    /// is the one the author put first.
    fn title(&self, element: &Element) -> Option<String> {
        let title = own(element, "title", self.ns).next()?;
        own(title, "langstring", self.ns)
            .map(|langstring| {
                langstring
                    .text
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .find(|text| !text.is_empty())
    }

    /// An element's `id`: present, an absolute IRI, and not used before.
    /// The identifiers become xAPI activity identifiers, which must be
    /// absolute (cmi5 requirement 3.0.0.0-1).
    fn identifier(&mut self, element: &Element, what: &str) -> Result<String, Refusal> {
        let id =
            attribute(element, "id").ok_or_else(|| invalid(format!("a <{what}> has no id")))?;
        if !is_absolute_iri(id) {
            return Err(invalid(format!(
                "the <{what}> id {id:?} is not an absolute IRI"
            )));
        }
        if !self.seen.insert(id.to_owned()) {
            return Err(invalid(format!("the id {id} is used more than once")));
        }
        Ok(id.to_owned())
    }
}

/// An attribute's trimmed value, if it has one.
fn attribute<'e>(element: &'e Element, name: &str) -> Option<&'e str> {
    element
        .attribute(name)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

/// A `masteryScore`: an `xs:decimal` from 0 to 1. `xs:decimal` is digits
/// with an optional sign and point, so the exponents, infinities and NaN
/// that Rust would also parse are turned away first.
fn mastery_score(text: &str) -> Option<f64> {
    let digits = text.strip_prefix(['+', '-']).unwrap_or(text);
    let decimal = digits.chars().any(|c| c.is_ascii_digit())
        && digits.chars().all(|c| c.is_ascii_digit() || c == '.')
        && digits.matches('.').count() <= 1;
    if !decimal {
        return None;
    }
    text.parse::<f64>()
        .ok()
        .filter(|score| (0.0..=1.0).contains(score))
}

/// A scheme, a colon, something after it, and no white space.
fn is_absolute_iri(id: &str) -> bool {
    let Some((scheme, rest)) = id.split_once(':') else {
        return false;
    };
    scheme.starts_with(|first: char| first.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-'))
        && !rest.is_empty()
        && !id.chars().any(char::is_whitespace)
}

/// The query names the launching system adds to every unit's URL (cmi5
/// section 8.1). A unit URL may carry a query of its own, but not with
/// these names.
const LAUNCH_PARAMETER_NAMES: [&str; 5] =
    ["endpoint", "fetch", "actor", "registration", "activityId"];

/// The first cmi5 launch parameter name the query in `suffix` already uses.
fn launch_parameter_clash(suffix: &str) -> Option<&'static str> {
    let query = suffix.split('#').next().unwrap_or_default();
    let query = query.strip_prefix('?')?;
    query.split('&').find_map(|pair| {
        let name = pair.split('=').next().unwrap_or_default();
        let name = percent_decode_str(name).decode_utf8_lossy();
        LAUNCH_PARAMETER_NAMES
            .into_iter()
            .find(|reserved| *reserved == name)
    })
}

fn has_launch(activity: &Activity) -> bool {
    activity.launch.is_some() || activity.children.iter().any(has_launch)
}

fn invalid(detail: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::ManifestUnparseable, detail)
}
