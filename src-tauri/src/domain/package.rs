//! Standards packages as the app stores and reports them. Mirrored in
//! `src/types/index.ts`. The parsed description is the shared crate's
//! [`PackageDescription`]; this module adds where the archive is kept.

use alexandria_studio::package::{Edition, PackageDescription, Standard};
use serde::{Deserialize, Serialize};

/// Where a package's archive, the kept copy, lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArchiveLocation {
    /// A file in the profile's packages directory. The node never serves
    /// it and nothing announces it: a draft's package stays on the device.
    Draft,
    /// The content store the node serves from, pinned so it is never
    /// evicted. A published course's package, or one received with a course.
    Store,
}

impl ArchiveLocation {
    pub fn as_str(self) -> &'static str {
        match self {
            ArchiveLocation::Draft => "draft",
            ArchiveLocation::Store => "store",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "draft" => Some(ArchiveLocation::Draft),
            "store" => Some(ArchiveLocation::Store),
            _ => None,
        }
    }
}

/// One row of the `packages` table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Package {
    /// BLAKE3 of the archive's own bytes, hex. A package element's content hash.
    pub hash: String,
    pub standard: Standard,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edition: Option<Edition>,
    pub title: String,
    /// What import parsed: the activity tree, launches, file count and size.
    pub description: PackageDescription,
    /// Size of the archive in bytes.
    pub archive_bytes: u64,
    pub location: ArchiveLocation,
    pub imported_at: String,
    /// The last time the package was extracted for a launch.
    pub last_opened_at: Option<String>,
}

/// What opening a package file from disk made: the package and the draft
/// course around it, which has one chapter holding one package element.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpenedPackageFile {
    pub package: Package,
    pub course_id: String,
    pub chapter_id: String,
    pub element_id: String,
}

/// A package file opened to learn from: the draft course around it and the
/// learner's local enrolment in that course, which the player needs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpenedPackageCourse {
    #[serde(flatten)]
    pub opened: OpenedPackageFile,
    pub enrollment_id: String,
}

/// Extraction progress, sent after each file and about once per MiB inside a
/// large file. The totals come from the stored description.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PackageExtractProgress {
    pub hash: String,
    pub files_done: u64,
    pub files_total: u64,
    pub bytes_done: u64,
    pub bytes_total: u64,
}
