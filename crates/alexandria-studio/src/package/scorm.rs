//! What the SCORM versions share: telling them apart, and turning a
//! resource's address into a file in the archive.

use std::io::{Read, Seek};

use percent_encoding::percent_decode_str;

use super::archive::Archive;
use super::xml::Element;
use super::{Edition, Refusal, RefusalCode, Standard};

/// The ADL extension namespace of SCORM 1.2.
const ADLCP_1_2: &str = "http://www.adlnet.org/xsd/adlcp_rootv1p2";
/// The namespaces only a SCORM 2004 manifest uses: the ADL extensions, ADL
/// sequencing and navigation, and IMS simple sequencing.
const SCORM_2004: [&str; 4] = [
    "http://www.adlnet.org/xsd/adlcp_v1p3",
    "http://www.adlnet.org/xsd/adlseq_v1p3",
    "http://www.adlnet.org/xsd/adlnav_v1p3",
    "http://www.imsglobal.org/xsd/imsss",
];

/// Which SCORM a manifest is.
///
/// The ADL namespace in use decides, because it is what fixes the names the
/// rest of the manifest is written in. The declared schema version supplies
/// the edition, and stands in when a manifest uses no ADL namespace at all.
pub(crate) fn detect(manifest: &Element) -> Result<(Standard, Option<Edition>), Refusal> {
    if manifest.name != "manifest" {
        return Err(Refusal::new(
            RefusalCode::ManifestUnparseable,
            format!(
                "the manifest's root element is <{}>, not <manifest>",
                manifest.name
            ),
        ));
    }
    let declared = manifest
        .child("metadata")
        .and_then(|metadata| metadata.child_text("schemaversion"))
        .unwrap_or_default()
        .to_ascii_lowercase();
    let edition = if declared.contains("4th") {
        Some(Edition::Fourth)
    } else if declared.contains("3rd") {
        Some(Edition::Third)
    } else if declared.contains("2nd") || declared == "cam 1.3" {
        Some(Edition::Second)
    } else {
        None
    };

    if manifest.uses_namespace(&SCORM_2004) {
        Ok((Standard::Scorm2004, edition))
    } else if manifest.uses_namespace(&[ADLCP_1_2]) || declared == "1.2" {
        Ok((Standard::Scorm12, None))
    } else if edition.is_some() || declared.contains("2004") {
        Ok((Standard::Scorm2004, edition))
    } else {
        Err(Refusal::new(
            RefusalCode::UnsupportedStandard,
            "the manifest does not say it is SCORM 1.2 or SCORM 2004",
        ))
    }
}

/// The file an address launches, and whatever follows the path in it.
pub(crate) struct Target {
    /// Path inside the archive, in the archive's own spelling.
    pub path: String,
    /// The address's query and fragment, from the first `?` or `#` on.
    pub suffix: String,
}

/// Finds the archive file a launch address points at.
///
/// `address` is the resource's `href` with every `xml:base` above it already
/// put in front. Anything that is not a plain relative address is refused as
/// remote: a package may only launch what it carries.
pub(crate) fn launch_target<R: Read + Seek>(
    address: &str,
    archive: &Archive<R>,
) -> Result<Target, Refusal> {
    let address = address.trim().replace('\\', "/");
    if is_remote(&address) {
        return Err(Refusal::new(
            RefusalCode::LaunchResourceRemote,
            format!("{address} is outside the package"),
        ));
    }
    let missing = || {
        Refusal::new(
            RefusalCode::LaunchResourceMissing,
            format!("{address} is not in the archive"),
        )
    };

    let cut = address.find(['?', '#']).unwrap_or(address.len());
    let (path, suffix) = address.split_at(cut);
    let path = percent_decode_str(path)
        .decode_utf8()
        .map_err(|_| missing())?;
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop().ok_or_else(missing)?;
            }
            part => parts.push(part),
        }
    }
    let path = archive.spelling(&parts.join("/")).ok_or_else(missing)?;
    Ok(Target {
        path: path.to_owned(),
        suffix: suffix.to_owned(),
    })
}

/// Joins what follows the path in a resource's address with an item's
/// `parameters`, the way the SCORM content aggregation model lays out: a
/// leading `?` or `&` on the parameters is dropped, a query is added with `?`
/// or `&` as the address needs, and a fragment is added only where the
/// address has none.
pub(crate) fn launch_parameters(suffix: &str, parameters: &str) -> Option<String> {
    let parameters = parameters.trim().trim_start_matches(['?', '&']);
    let (query, fragment) = match suffix.find('#') {
        Some(at) => suffix.split_at(at),
        None => (suffix, ""),
    };
    let joined = if parameters.is_empty() {
        suffix.to_owned()
    } else if parameters.starts_with('#') {
        if fragment.is_empty() {
            format!("{query}{parameters}")
        } else {
            suffix.to_owned()
        }
    } else {
        let separator = if query.is_empty() { '?' } else { '&' };
        let fragment = if parameters.contains('#') {
            ""
        } else {
            fragment
        };
        format!("{query}{separator}{parameters}{fragment}")
    };
    Some(joined).filter(|joined| !joined.is_empty())
}

/// True for an address with a scheme (`https:`, `file:`, `javascript:`, a
/// Windows drive) or a host (`//example.org/…`).
fn is_remote(address: &str) -> bool {
    if address.starts_with("//") {
        return true;
    }
    let Some((scheme, _)) = address.split_once(':') else {
        return false;
    };
    scheme.starts_with(|first: char| first.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-'))
}
