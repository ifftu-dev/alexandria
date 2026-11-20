//! Standards package import.
//!
//! A standards package is a SCORM 1.2, SCORM 2004 or cmi5 content package:
//! one zip archive holding a manifest and the files it launches. This module
//! is the desktop-free half of handling one (ADR 0004). It has two entry
//! points and no storage:
//!
//! - [`import_package`] reads an archive and returns either a
//!   [`PackageDescription`] or a [`Refusal`] carrying a stable code.
//! - [`extract_package`] writes an archive's files under a directory the
//!   caller names, refusing anything that would land outside it.
//!
//! Both treat the archive as hostile and both enforce the same [`Limits`].
//! A package that imports under some limits extracts under the same limits:
//! import decompresses every entry to count what is really there, because the
//! sizes an archive declares are whatever its author typed.
//!
//! SCORM 1.2 and SCORM 2004 (2nd, 3rd and 4th Editions) are parsed; a SCORM
//! 2004 activity carries its [`sequencing`] definition. cmi5 is recognised
//! and refused with [`RefusalCode::UnsupportedStandard`] until its importer
//! exists.

mod archive;
mod scorm;
mod scorm12;
mod scorm2004;
pub mod sequencing;
mod xml;

use std::fmt;
use std::io::{Read, Seek};

use serde::{Deserialize, Serialize};

pub use archive::{extract_package, ExtractError, ExtractProgress, ExtractSummary};

/// The caps import and extraction hold an archive to.
///
/// [`Limits::default`] is the starting point the learning standards spec
/// names. A caller that needs to admit a larger package passes its own value;
/// nothing else in this module holds a size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Total size of every file once decompressed.
    pub max_uncompressed_bytes: u64,
    /// Entries in the archive, files and folders together.
    pub max_entries: usize,
    /// Total decompressed size divided by the size of the archive itself.
    /// Measured against the whole archive, not per entry, so one blank image
    /// does not sink a package but entries that share compressed data do.
    pub max_compression_ratio: u64,
}

impl Limits {
    /// 1 GiB uncompressed, 20,000 entries, 200:1.
    pub const STARTING: Limits = Limits {
        max_uncompressed_bytes: 1 << 30,
        max_entries: 20_000,
        max_compression_ratio: 200,
    };
}

impl Default for Limits {
    fn default() -> Self {
        Self::STARTING
    }
}

/// Which standard a package follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Standard {
    Scorm12,
    Scorm2004,
    Cmi5,
}

impl Standard {
    pub fn as_str(self) -> &'static str {
        match self {
            Standard::Scorm12 => "scorm12",
            Standard::Scorm2004 => "scorm2004",
            Standard::Cmi5 => "cmi5",
        }
    }
}

impl fmt::Display for Standard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The edition of SCORM 2004 a manifest declares. The other standards have
/// no editions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Edition {
    #[serde(rename = "2nd")]
    Second,
    #[serde(rename = "3rd")]
    Third,
    #[serde(rename = "4th")]
    Fourth,
}

impl Edition {
    pub fn as_str(self) -> &'static str {
        match self {
            Edition::Second => "2nd",
            Edition::Third => "3rd",
            Edition::Fourth => "4th",
        }
    }
}

impl fmt::Display for Edition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What import learned about a package: enough to list its lessons, launch
/// each one and hand the run-time its starting data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct PackageDescription {
    pub standard: Standard,
    /// Set for SCORM 2004 only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edition: Option<Edition>,
    /// The package's own identifier: the manifest identifier for SCORM.
    pub identifier: String,
    pub title: String,
    /// The activity tree. The root is the package's default organization and
    /// is never launchable itself.
    pub tree: Activity,
    /// Files in the archive.
    pub files: u64,
    /// Their total size once decompressed, as measured, not as declared.
    pub uncompressed_bytes: u64,
}

impl PackageDescription {
    /// Every launchable activity, in the order a learner meets them.
    pub fn launchable_activities(&self) -> Vec<&Activity> {
        let mut found = Vec::new();
        self.tree.collect_launchable(&mut found);
        found
    }
}

/// One node of the activity tree: a lesson when it has a [`Launch`], a
/// heading over its children when it does not.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Activity {
    /// Unique within the package. Run-time state is keyed by it.
    pub id: String,
    pub title: String,
    /// False when the author asked for the activity to be left out of the
    /// table of contents.
    pub visible: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub launch: Option<Launch>,
    /// SCORM 1.2 `adlcp:prerequisites`, as the author wrote it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prerequisites: Option<String>,
    /// SCORM 2004 `imsss:sequencing`, with any sequencing collection applied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sequencing: Option<sequencing::Sequencing>,
    /// SCORM 2004 `adlcp:completionThreshold`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion_threshold: Option<sequencing::CompletionThreshold>,
    /// SCORM 2004 `adlnav:hideLMSUI`: navigation controls the player should
    /// not draw while this activity is delivered.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hide_lms_ui: Vec<sequencing::NavigationControl>,
    /// SCORM 2004 `adlcp:data`: the shared data stores the lesson may use.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shared_data: Vec<sequencing::SharedDataMap>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Activity>,
}

impl Activity {
    fn collect_launchable<'a>(&'a self, found: &mut Vec<&'a Activity>) {
        if self.launch.is_some() {
            found.push(self);
        }
        for child in &self.children {
            child.collect_launchable(found);
        }
    }
}

/// How to start one activity, and what the manifest says the run-time should
/// be given when it does.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Launch {
    /// The file to open, as a path inside the archive: `/`-separated, decoded,
    /// and spelled the way the archive spells it. Extraction writes the file
    /// at this same path under its target directory.
    pub path: String,
    /// What follows the path in the launch address, beginning with `?` or
    /// `#`: the resource's own query and fragment joined with the item's
    /// parameters. Append it unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameters: Option<String>,
    pub kind: LaunchKind,
    /// The text the run-time hands the content as launch data
    /// (SCORM 1.2 `adlcp:datafromlms`, SCORM 2004 `adlcp:dataFromLMS`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub launch_data: Option<String>,
    /// The passing score, on the scale of the package's own standard:
    /// 0 to 100 for SCORM 1.2 (`adlcp:masteryscore`); -1 to 1 for SCORM 2004
    /// (`cmi.scaled_passing_score`: the primary objective's minimum
    /// normalized measure, set only when that objective is satisfied by
    /// measure).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mastery_score: Option<f64>,
    /// SCORM 1.2 `adlcp:maxtimeallowed`, as the author wrote it. SCORM 2004:
    /// the activity's attempt absolute duration limit, an XML Schema
    /// duration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_time_allowed: Option<String>,
    /// SCORM 1.2 `adlcp:timelimitaction` or SCORM 2004
    /// `adlcp:timeLimitAction`, as the author wrote it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_limit_action: Option<String>,
}

/// What kind of thing a launch opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaunchKind {
    /// Content that talks to the run-time API and reports its own result.
    Sco,
    /// Content that is only shown. It reports nothing.
    Asset,
}

/// Why a package was turned away. The names are stable: they are stored,
/// shown and matched on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefusalCode {
    /// The bytes are not a zip archive this module can read. Covers a
    /// corrupt, encrypted or exotically compressed entry too.
    NotAnArchive,
    /// No manifest at the top of the archive.
    NoManifest,
    /// The manifest is not well-formed, or lacks something its standard
    /// requires.
    ManifestUnparseable,
    /// A standard or edition this build does not play.
    UnsupportedStandard,
    /// An activity launches a file the archive does not hold.
    LaunchResourceMissing,
    /// An activity launches something outside the archive.
    LaunchResourceRemote,
    /// An entry is a link, or its path is absolute or climbs out of the
    /// package.
    UnsafeEntryPath,
    /// The archive is over a size, count or compression limit.
    LimitExceeded,
}

impl RefusalCode {
    pub fn as_str(self) -> &'static str {
        match self {
            RefusalCode::NotAnArchive => "not_an_archive",
            RefusalCode::NoManifest => "no_manifest",
            RefusalCode::ManifestUnparseable => "manifest_unparseable",
            RefusalCode::UnsupportedStandard => "unsupported_standard",
            RefusalCode::LaunchResourceMissing => "launch_resource_missing",
            RefusalCode::LaunchResourceRemote => "launch_resource_remote",
            RefusalCode::UnsafeEntryPath => "unsafe_entry_path",
            RefusalCode::LimitExceeded => "limit_exceeded",
        }
    }
}

impl fmt::Display for RefusalCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A package turned away, with the reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[error("{code}: {detail}")]
pub struct Refusal {
    pub code: RefusalCode,
    /// What exactly was wrong, in English, for the person who has to fix the
    /// package. Match on `code`, never on this.
    pub detail: String,
    /// The standard, when import got far enough to recognise one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub standard: Option<Standard>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edition: Option<Edition>,
}

impl Refusal {
    pub(crate) fn new(code: RefusalCode, detail: impl Into<String>) -> Self {
        Self {
            code,
            detail: detail.into(),
            standard: None,
            edition: None,
        }
    }

    pub(crate) fn of(mut self, standard: Standard, edition: Option<Edition>) -> Self {
        self.standard = Some(standard);
        self.edition = edition;
        self
    }
}

/// The largest manifest import will read. A manifest is a table of contents;
/// one this big is not a table of contents.
const MAX_MANIFEST_BYTES: u64 = 16 << 20;

const SCORM_MANIFEST: &str = "imsmanifest.xml";
const CMI5_STRUCTURE: &str = "cmi5.xml";

/// Reads a standards package and describes it, or says why not.
///
/// Pure: the archive is only read, and nothing is written anywhere. `archive`
/// is any seekable source, so a caller holding the bytes passes
/// `std::io::Cursor::new(bytes)` and a caller holding a large file passes the
/// file. A source that fails to read is reported as
/// [`RefusalCode::NotAnArchive`].
///
/// Every entry is decompressed once to measure it, so the cost grows with
/// the package's real size; run it off the interface thread.
pub fn import_package<R: Read + Seek>(
    archive: R,
    limits: &Limits,
) -> Result<PackageDescription, Refusal> {
    let mut archive = archive::Archive::open(archive, limits)?;

    // A SCORM manifest decides when both files are present.
    let Some(manifest) = archive.file(SCORM_MANIFEST) else {
        if archive.file(CMI5_STRUCTURE).is_some() {
            return Err(unsupported(Standard::Cmi5, None));
        }
        return Err(no_manifest(&archive));
    };

    let unparseable = |detail| Refusal::new(RefusalCode::ManifestUnparseable, detail);
    let bytes = archive.read(manifest, MAX_MANIFEST_BYTES)?;
    let text = xml::decode(&bytes).map_err(unparseable)?;
    let root = xml::parse(&text).map_err(unparseable)?;

    let (standard, edition) = scorm::detect(&root)?;
    let parsed = match standard {
        Standard::Scorm12 => scorm12::parse(&root, &archive),
        Standard::Scorm2004 => scorm2004::parse(&root, &archive),
        Standard::Cmi5 => Err(unsupported(standard, edition)),
    }
    .map_err(|refusal| refusal.of(standard, edition))?;

    let measured = archive
        .measure(limits)
        .map_err(|refusal| refusal.of(standard, edition))?;

    Ok(PackageDescription {
        standard,
        edition,
        identifier: parsed.identifier,
        title: parsed.title,
        tree: parsed.tree,
        files: measured.files,
        uncompressed_bytes: measured.bytes,
    })
}

/// A standard import recognises and has no importer for.
fn unsupported(standard: Standard, edition: Option<Edition>) -> Refusal {
    let name = match (standard, edition) {
        (Standard::Scorm12, _) => "SCORM 1.2".to_owned(),
        (Standard::Scorm2004, Some(edition)) => format!("SCORM 2004 {edition} Edition"),
        (Standard::Scorm2004, None) => "SCORM 2004".to_owned(),
        (Standard::Cmi5, _) => "cmi5".to_owned(),
    };
    Refusal::new(
        RefusalCode::UnsupportedStandard,
        format!("{name} packages are not supported yet"),
    )
    .of(standard, edition)
}

/// "No manifest", with a pointer when the usual mistake was made: zipping the
/// folder instead of its contents.
fn no_manifest<R: Read + Seek>(archive: &archive::Archive<R>) -> Refusal {
    let nested = archive.file_paths().find(|path| {
        path.rsplit('/')
            .next()
            .is_some_and(|name| name == SCORM_MANIFEST || name == CMI5_STRUCTURE)
    });
    let detail = match nested {
        Some(path) => format!(
            "no {SCORM_MANIFEST} or {CMI5_STRUCTURE} at the top of the archive; \
             found {path}, which must be at the top"
        ),
        None => format!("no {SCORM_MANIFEST} or {CMI5_STRUCTURE} at the top of the archive"),
    };
    Refusal::new(RefusalCode::NoManifest, detail)
}

/// What a manifest parser hands back to [`import_package`].
pub(crate) struct ParsedManifest {
    pub identifier: String,
    pub title: String,
    pub tree: Activity,
}
