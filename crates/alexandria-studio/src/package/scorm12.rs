//! SCORM 1.2 manifests: the shared content package walk, plus the
//! `adlcp:` item elements SCORM 1.2 hands its run-time.

use std::io::{Read, Seek};

use super::archive::Archive;
use super::scorm::{invalid, parse_content_package, Version};
use super::xml::Element;
use super::{Activity, ParsedManifest, Refusal};

pub(crate) fn parse<R: Read + Seek>(
    manifest: &Element,
    archive: &Archive<R>,
) -> Result<ParsedManifest, Refusal> {
    parse_content_package(manifest, archive, &mut Scorm12)
}

struct Scorm12;

impl Version for Scorm12 {
    fn read(&mut self, item: &Element, activity: &mut Activity) -> Result<(), Refusal> {
        activity.prerequisites = item.child_text("prerequisites").map(str::to_owned);
        let Some(launch) = activity.launch.as_mut() else {
            return Ok(());
        };
        launch.mastery_score = match item.child_text("masteryscore") {
            None => None,
            Some(text) => Some(
                text.parse::<f64>()
                    .ok()
                    .filter(|score| (0.0..=100.0).contains(score))
                    .ok_or_else(|| {
                        invalid(format!(
                            "item {} has mastery score {text:?}; it must be a number from 0 to 100",
                            activity.id
                        ))
                    })?,
            ),
        };
        launch.launch_data = item.child_text("datafromlms").map(str::to_owned);
        launch.max_time_allowed = item.child_text("maxtimeallowed").map(str::to_owned);
        launch.time_limit_action = item.child_text("timelimitaction").map(str::to_owned);
        Ok(())
    }
}
