//! SCORM 1.2 manifests: the default organization becomes the activity tree,
//! each item that names a resource becomes a launchable activity.

use std::collections::{HashMap, HashSet};
use std::io::{Read, Seek};

use super::archive::Archive;
use super::scorm::{launch_parameters, launch_target};
use super::xml::Element;
use super::{Activity, Launch, LaunchKind, ParsedManifest, Refusal, RefusalCode};

pub(crate) fn parse<R: Read + Seek>(
    manifest: &Element,
    archive: &Archive<R>,
) -> Result<ParsedManifest, Refusal> {
    let organizations = manifest
        .child("organizations")
        .ok_or_else(|| invalid("the manifest has no <organizations>"))?;
    // A package may carry several organizations; the one it marks as default
    // is the one a learner is given. A missing or dangling default falls back
    // to the first.
    let organization = organizations
        .attribute("default")
        .and_then(|default| {
            organizations
                .children_named("organization")
                .find(|organization| organization.attribute("identifier") == Some(default))
        })
        .or_else(|| organizations.children_named("organization").next())
        .ok_or_else(|| invalid("the manifest has no organization, so there is nothing to play"))?;

    let resources = manifest.child("resources");
    let mut context = Context {
        archive,
        base: format!(
            "{}{}",
            manifest.xml_base(),
            resources.map_or("", Element::xml_base)
        ),
        resources: resources
            .into_iter()
            .flat_map(|resources| resources.children_named("resource"))
            .filter_map(|resource| Some((resource.attribute("identifier")?.trim(), resource)))
            .collect(),
        seen: HashSet::new(),
    };

    let id = identifier(organization, "organization", &mut context.seen)?;
    let children = organization
        .children_named("item")
        .map(|item| activity(item, &mut context))
        .collect::<Result<Vec<_>, _>>()?;
    let identifier = manifest
        .attribute("identifier")
        .unwrap_or_default()
        .trim()
        .to_owned();
    let title = title(organization).unwrap_or_else(|| identifier.clone());
    let tree = Activity {
        id,
        title: title.clone(),
        visible: true,
        launch: None,
        prerequisites: None,
        children,
        cmi5: None,
    };
    if !has_launch(&tree) {
        return Err(Refusal::new(
            RefusalCode::LaunchResourceMissing,
            "no item in the default organization launches anything",
        ));
    }
    Ok(ParsedManifest {
        identifier,
        title,
        tree,
    })
}

struct Context<'a, R> {
    archive: &'a Archive<R>,
    /// The `xml:base` of the manifest and of `<resources>`, joined.
    base: String,
    resources: HashMap<&'a str, &'a Element>,
    /// Identifiers already used in the tree.
    seen: HashSet<String>,
}

fn activity<R: Read + Seek>(
    item: &Element,
    context: &mut Context<'_, R>,
) -> Result<Activity, Refusal> {
    let id = identifier(item, "item", &mut context.seen)?;
    let launch = match item.attribute("identifierref").map(str::trim) {
        None | Some("") => None,
        Some(reference) => Some(launch(item, &id, reference, context)?),
    };
    let children = item
        .children_named("item")
        .map(|child| activity(child, context))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Activity {
        title: title(item).unwrap_or_else(|| id.clone()),
        visible: !item
            .attribute("isvisible")
            .is_some_and(|visible| visible.trim().eq_ignore_ascii_case("false")),
        launch,
        prerequisites: item.child_text("prerequisites").map(str::to_owned),
        children,
        cmi5: None,
        id,
    })
}

fn launch<R: Read + Seek>(
    item: &Element,
    id: &str,
    reference: &str,
    context: &Context<'_, R>,
) -> Result<Launch, Refusal> {
    let resource = context.resources.get(reference).ok_or_else(|| {
        invalid(format!(
            "item {id} refers to resource {reference}, which the manifest does not define"
        ))
    })?;
    let href = resource
        .attribute("href")
        .map(str::trim)
        .filter(|href| !href.is_empty())
        .ok_or_else(|| {
            Refusal::new(
                RefusalCode::LaunchResourceMissing,
                format!("item {id} launches resource {reference}, which names no file"),
            )
        })?;
    let address = format!("{}{}{href}", context.base, resource.xml_base());
    let target = launch_target(&address, context.archive).map_err(|mut refusal| {
        refusal.detail = format!("item {id}: {}", refusal.detail);
        refusal
    })?;

    // The standard requires `adlcp:scormtype` and gives no default. Where it
    // is missing the resource is taken for an asset: it is shown, and nothing
    // is expected of it.
    let kind = match resource.attribute("scormtype").map(str::trim) {
        None => LaunchKind::Asset,
        Some(kind) if kind.eq_ignore_ascii_case("asset") => LaunchKind::Asset,
        Some(kind) if kind.eq_ignore_ascii_case("sco") => LaunchKind::Sco,
        Some(other) => {
            return Err(invalid(format!(
                "resource {reference} has scormtype {other:?}; it must be sco or asset"
            )))
        }
    };
    let mastery_score = match item.child_text("masteryscore") {
        None => None,
        Some(text) => Some(
            text.parse::<f64>()
                .ok()
                .filter(|score| (0.0..=100.0).contains(score))
                .ok_or_else(|| {
                    invalid(format!(
                        "item {id} has mastery score {text:?}; it must be a number from 0 to 100"
                    ))
                })?,
        ),
    };

    Ok(Launch {
        path: target.path,
        parameters: launch_parameters(
            &target.suffix,
            item.attribute("parameters").unwrap_or_default(),
        ),
        kind,
        launch_data: item.child_text("datafromlms").map(str::to_owned),
        mastery_score,
        max_time_allowed: item.child_text("maxtimeallowed").map(str::to_owned),
        time_limit_action: item.child_text("timelimitaction").map(str::to_owned),
    })
}

/// The element's `<title>` on one line, however the manifest was indented.
fn title(element: &Element) -> Option<String> {
    let words: Vec<&str> = element.child("title")?.text.split_whitespace().collect();
    Some(words.join(" ")).filter(|title| !title.is_empty())
}

/// An element's identifier, which must be present and not used before:
/// run-time state is stored against it.
fn identifier(
    element: &Element,
    what: &str,
    seen: &mut HashSet<String>,
) -> Result<String, Refusal> {
    let id = element
        .attribute("identifier")
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .ok_or_else(|| invalid(format!("an <{what}> has no identifier")))?;
    if !seen.insert(id.to_owned()) {
        return Err(invalid(format!(
            "the identifier {id} is used more than once"
        )));
    }
    Ok(id.to_owned())
}

fn has_launch(activity: &Activity) -> bool {
    activity.launch.is_some() || activity.children.iter().any(has_launch)
}

fn invalid(detail: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::ManifestUnparseable, detail)
}
