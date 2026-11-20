//! Standards packages on this device: storage, the extraction cache and
//! opening a package file from disk.
//!
//! Parsing and validation are the shared crate's
//! ([`alexandria_studio::package`], ADR 0004); this module adds where the
//! bytes live.
//!
//! - **The archive is the kept copy.** It is addressed by the BLAKE3 hash of
//!   its own bytes, never encrypted with the device key, so the hash a course
//!   document carries is the hash every device computes.
//! - **A draft's archive stays off the served store.** The content node
//!   answers any request for a hash it holds, so "never served" means the
//!   archive is not in the store: it is a file in the profile's packages
//!   directory ([`ArchiveLocation::Draft`]) until [`publish`] moves it into
//!   the store with a pin that eviction never removes.
//! - **Extracted files are a cache.** They live under the profile's package
//!   cache, one directory per hash, are dropped on completion or after
//!   [`IDLE_DROP_AFTER`] without a launch, and are rebuilt from the archive
//!   by [`ensure_extracted`]. Extraction runs off the calling thread and
//!   reports progress.
//!
//! Database work goes through [`PackageDb`] so the same functions run on
//! the executor in commands and on a plain database in tests; nothing holds
//! the database across an await.

mod archive;
pub mod attempts;
mod cache;
mod draft;
pub mod preview;
mod rows;

#[cfg(test)]
pub(crate) mod tests;

use std::future::Future;
use std::path::{Path, PathBuf};

use alexandria_studio::package::{ExtractProgress, Limits, Refusal};
use chrono::{DateTime, TimeDelta, Utc};
use iroh_blobs::api::blobs::BlobStatus;

use crate::content_store::content::{self, ContentError};
use crate::content_store::node::ContentNode;
use crate::db::executor::{DatabaseExecutor, DatabaseWorkload};
use crate::db::Database;
use crate::domain::package::{ArchiveLocation, OpenedPackageFile, Package, PackageExtractProgress};
use crate::profile::manager::ProfilePaths;
use crate::profile::scope::ProfileLease;

/// The limits every import and every extraction of a package use. One value
/// for both, so an archive that imported is never refused at extraction.
pub const LIMITS: Limits = Limits::STARTING;

/// Why a package operation failed.
#[derive(Debug, thiserror::Error)]
pub enum PackageError {
    /// The archive is not a package this app accepts. Displays as
    /// `code: detail`; match on [`Refusal::code`], never on the detail.
    #[error(transparent)]
    Refused(Refusal),
    #[error("invalid package hash: {0}")]
    InvalidHash(String),
    #[error("package not found: {0}")]
    NotFound(String),
    #[error("content store: {0}")]
    Content(#[from] crate::content_store::content::ContentError),
    #[error("package files: {0}")]
    Io(#[from] std::io::Error),
    /// Opening a package file as a course needs the local author.
    #[error("no identity found — generate a wallet first: {0}")]
    NoIdentity(String),
    #[error("{0}")]
    Database(String),
}

impl From<rusqlite::Error> for PackageError {
    fn from(error: rusqlite::Error) -> Self {
        PackageError::Database(error.to_string())
    }
}

/// Runs one database step whose own failures are typed: an error from the
/// step comes back as it is; a failure to reach the database at all is
/// [`PackageError::Database`].
pub(crate) async fn run_step<T, F>(
    db: &impl PackageDb,
    label: &'static str,
    step: F,
) -> Result<T, PackageError>
where
    T: Send + 'static,
    F: FnOnce(&Database) -> Result<T, PackageError> + Send + 'static,
{
    db.run(label, move |db| Ok(step(db)))
        .await
        .map_err(PackageError::Database)?
}

/// The profile's two package directories.
#[derive(Debug, Clone)]
pub struct PackageDirs {
    /// Draft archives, `<hash>.zip`. Never served.
    pub archives: PathBuf,
    /// Extracted files, `<hash>/<archive path>`.
    pub cache: PathBuf,
}

impl PackageDirs {
    pub fn for_profile(paths: &ProfilePaths) -> Self {
        Self {
            archives: paths.packages_dir.clone(),
            cache: paths.package_cache_dir.clone(),
        }
    }

    pub fn ensure(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.archives)?;
        std::fs::create_dir_all(&self.cache)
    }
}

/// Somewhere to run database work: the executor in commands, a plain
/// database in tests.
pub trait PackageDb: Sync {
    fn run<T, F>(
        &self,
        label: &'static str,
        operation: F,
    ) -> impl Future<Output = Result<T, String>> + Send
    where
        T: Send + 'static,
        F: FnOnce(&Database) -> Result<T, String> + Send + 'static;
}

/// [`PackageDb`] over the profile's database executor, owned: for work that
/// outlives the command that started it (a cmi5 session's endpoint).
pub(crate) struct OwnedExecutorDb {
    pub(crate) executor: DatabaseExecutor,
    pub(crate) lease: ProfileLease,
    pub(crate) workload: DatabaseWorkload,
}

impl PackageDb for OwnedExecutorDb {
    fn run<T, F>(
        &self,
        label: &'static str,
        operation: F,
    ) -> impl Future<Output = Result<T, String>> + Send
    where
        T: Send + 'static,
        F: FnOnce(&Database) -> Result<T, String> + Send + 'static,
    {
        self.executor
            .execute(self.workload, self.lease.clone(), label, operation)
    }
}

/// [`PackageDb`] over the profile's database executor.
pub(crate) struct ExecutorDb<'a> {
    pub(crate) executor: &'a DatabaseExecutor,
    pub(crate) lease: ProfileLease,
    pub(crate) workload: DatabaseWorkload,
}

impl PackageDb for ExecutorDb<'_> {
    fn run<T, F>(
        &self,
        label: &'static str,
        operation: F,
    ) -> impl Future<Output = Result<T, String>> + Send
    where
        T: Send + 'static,
        F: FnOnce(&Database) -> Result<T, String> + Send + 'static,
    {
        self.executor
            .execute(self.workload, self.lease.clone(), label, operation)
    }
}

/// The profile's shared database handle, as profile start holds it.
impl PackageDb for std::sync::Mutex<Option<Database>> {
    fn run<T, F>(
        &self,
        _label: &'static str,
        operation: F,
    ) -> impl Future<Output = Result<T, String>> + Send
    where
        T: Send + 'static,
        F: FnOnce(&Database) -> Result<T, String> + Send + 'static,
    {
        let result = match self.lock() {
            Ok(guard) => match guard.as_ref() {
                Some(database) => operation(database),
                None => Err("no database open".to_string()),
            },
            Err(_) => Err("database lock poisoned".to_string()),
        };
        std::future::ready(result)
    }
}

impl PackageDb for std::sync::Mutex<Database> {
    fn run<T, F>(
        &self,
        _label: &'static str,
        operation: F,
    ) -> impl Future<Output = Result<T, String>> + Send
    where
        T: Send + 'static,
        F: FnOnce(&Database) -> Result<T, String> + Send + 'static,
    {
        let result = match self.lock() {
            Ok(database) => operation(&database),
            Err(_) => Err("database lock poisoned".to_string()),
        };
        std::future::ready(result)
    }
}

/// A package hash as a path component: 64 lowercase hex characters.
fn checked_hash(hash: &str) -> Result<String, PackageError> {
    let hash = hash.trim().to_ascii_lowercase();
    if hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(hash)
    } else {
        Err(PackageError::InvalidHash(hash))
    }
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, PackageError> + Send + 'static,
) -> Result<T, PackageError> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|e| PackageError::Io(std::io::Error::other(e)))?
}

/// Imports the archive at `source`, a file on disk: checks it, keeps a copy
/// as a draft archive and records its description. Importing the same bytes
/// again returns the existing record. Nothing is added to the content store
/// and nothing is announced.
pub async fn import(
    db: &impl PackageDb,
    dirs: &PackageDirs,
    source: &Path,
) -> Result<Package, PackageError> {
    let archives = dirs.archives.clone();
    let source = source.to_path_buf();
    let checked = blocking(move || archive::keep_draft(&archives, &source)).await?;

    let package = Package {
        hash: checked.hash,
        standard: checked.description.standard,
        edition: checked.description.edition,
        title: checked.description.title.clone(),
        description: checked.description,
        archive_bytes: checked.size,
        location: ArchiveLocation::Draft,
        imported_at: crate::clock::now_rfc3339(),
        last_opened_at: None,
    };
    let stored = run_step(db, "packages.import", move |db| {
        rows::insert(db.conn(), &package)
    })
    .await?;
    if stored.location == ArchiveLocation::Store {
        // The served store already holds these bytes; one copy is enough.
        archive::discard_draft(&dirs.archives, &stored.hash)?;
    }
    Ok(stored)
}

/// The recorded package with this hash, if any.
pub async fn get(db: &impl PackageDb, hash: &str) -> Result<Option<Package>, PackageError> {
    let hash = checked_hash(hash)?;
    run_step(db, "packages.get", move |db| rows::get(db.conn(), &hash)).await
}

async fn required(db: &impl PackageDb, hash: &str) -> Result<Package, PackageError> {
    get(db, hash)
        .await?
        .ok_or_else(|| PackageError::NotFound(hash.to_string()))
}

/// Imports the package file at `source` and returns the draft course around
/// it: one chapter holding one [`PACKAGE_ELEMENT_TYPE`] element. Opening the
/// same file again returns the same draft. The archive stays a draft file:
/// it is not added to the content store, so the node cannot serve it, and
/// nothing is announced. It leaves the device only if the learner publishes
/// the course.
///
/// [`PACKAGE_ELEMENT_TYPE`]: crate::domain::course_document::PACKAGE_ELEMENT_TYPE
pub async fn open_file(
    db: &impl PackageDb,
    dirs: &PackageDirs,
    source: &Path,
) -> Result<OpenedPackageFile, PackageError> {
    let package = import(db, dirs, source).await?;
    let for_draft = package.clone();
    let ids = run_step(db, "packages.open-file", move |db| {
        draft::find_or_create(db.conn(), &for_draft)
    })
    .await?;
    Ok(OpenedPackageFile {
        package,
        course_id: ids.course_id,
        chapter_id: ids.chapter_id,
        element_id: ids.element_id,
    })
}

/// A package's extracted files, ready to serve.
#[derive(Debug, Clone)]
pub struct Extracted {
    /// The package, with `last_opened_at` just set.
    pub package: Package,
    /// The directory holding the package's files at their archive paths:
    /// a lesson's file is `root.join(&launch.path)`.
    pub root: PathBuf,
}

/// Makes sure the package's files are in the extraction cache and returns
/// where. When they are already there this returns at once and reports no
/// progress; otherwise it extracts from the kept archive (the draft file, or
/// a copy exported from the content store) on a blocking thread, calling
/// `progress` there after each file and about once per MiB inside a large
/// one. Either way it records the launch time the idle check reads.
pub async fn ensure_extracted(
    db: &impl PackageDb,
    node: &ContentNode,
    dirs: &PackageDirs,
    hash: &str,
    mut progress: impl FnMut(PackageExtractProgress) + Send + 'static,
) -> Result<Extracted, PackageError> {
    let hash = checked_hash(hash)?;
    let package = required(db, &hash).await?;
    let root = cache::extracted_path(&dirs.cache, &hash);
    if !root.is_dir() {
        let (archive, exported) = match package.location {
            ArchiveLocation::Draft => (archive::draft_path(&dirs.archives, &hash), false),
            ArchiveLocation::Store => (export_from_store(node, &dirs.cache, &hash).await?, true),
        };
        let cache_dir = dirs.cache.clone();
        let for_report = hash.clone();
        let files_total = package.description.files;
        let bytes_total = package.description.uncompressed_bytes;
        blocking(move || {
            let mut report = |step: &ExtractProgress<'_>| {
                progress(PackageExtractProgress {
                    hash: for_report.clone(),
                    files_done: step.files_done,
                    files_total,
                    bytes_done: step.bytes_done,
                    bytes_total,
                })
            };
            let extracted = cache::extract(&cache_dir, &for_report, &archive, &mut report);
            if exported {
                let _ = std::fs::remove_file(&archive);
            }
            extracted
        })
        .await?;
    }
    let now = crate::clock::now_rfc3339();
    let package = run_step(db, "packages.opened", move |db| {
        rows::mark_opened(db.conn(), &hash, &now)?;
        rows::get(db.conn(), &hash)?.ok_or_else(|| PackageError::NotFound(hash.clone()))
    })
    .await?;
    Ok(Extracted { package, root })
}

/// Drops a package's extracted files; the archive is untouched and the next
/// [`ensure_extracted`] rebuilds them. Returns whether anything was there.
/// Do not call it while a launch of the package is open: callers go through
/// [`crate::package_serving::drop_unless_open`].
pub async fn drop_extracted(dirs: &PackageDirs, hash: &str) -> Result<bool, PackageError> {
    let hash = checked_hash(hash)?;
    let cache_dir = dirs.cache.clone();
    blocking(move || match cache::retire(&cache_dir, &hash)? {
        Some(_) => {
            cache::empty_trash(&cache_dir)?;
            Ok(true)
        }
        None => Ok(false),
    })
    .await
}

/// How long extracted files are kept without a launch.
pub const IDLE_DROP_AFTER: TimeDelta = TimeDelta::days(14);

/// The idle check: drops the extracted files of every package not launched
/// within [`IDLE_DROP_AFTER`] before `now`, and returns the hashes whose files
/// were dropped. Runs at profile start.
pub async fn drop_idle(
    db: &impl PackageDb,
    dirs: &PackageDirs,
    now: DateTime<Utc>,
) -> Result<Vec<String>, PackageError> {
    let cutoff = (now - IDLE_DROP_AFTER).to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let idle = run_step(db, "packages.idle", move |db| {
        rows::idle_since(db.conn(), &cutoff)
    })
    .await?;
    let cache_dir = dirs.cache.clone();
    blocking(move || {
        let mut dropped = Vec::new();
        for hash in idle {
            if cache::retire(&cache_dir, &hash)?.is_some() {
                dropped.push(hash);
            }
        }
        cache::empty_trash(&cache_dir)?;
        Ok(dropped)
    })
    .await
}

/// Removes what an interrupted import, export or extraction left behind.
/// Call at profile start, before any package work can begin.
pub fn clear_scratch(dirs: &PackageDirs) -> Result<(), PackageError> {
    cache::clear_scratch(&dirs.cache, &dirs.archives)?;
    Ok(())
}

/// Moves a draft package's archive into the served content store, pinned so
/// eviction never removes it, and deletes the draft file. The publish flow
/// calls this for every package element of a course being published, before
/// the course is announced. Idempotent: a package already in the store is
/// returned as it is.
pub async fn publish(
    db: &impl PackageDb,
    node: &ContentNode,
    dirs: &PackageDirs,
    hash: &str,
) -> Result<Package, PackageError> {
    let hash = checked_hash(hash)?;
    let package = required(db, &hash).await?;
    if package.location == ArchiveLocation::Store {
        return Ok(package);
    }

    let draft = archive::draft_path(&dirs.archives, &hash);
    let store = node
        .store()
        .await
        .map_err(|_| ContentError::NodeNotRunning)?;
    let added = store
        .add_path(&draft)
        .temp_tag()
        .await
        .map_err(|e| ContentError::Store(e.to_string()))?;
    if added.hash() != content::parse_hash(&hash)? {
        return Err(PackageError::Io(std::io::Error::other(format!(
            "the draft archive for {hash} no longer matches its hash"
        ))));
    }
    store
        .tags()
        .set(&hash, added.hash_and_format())
        .await
        .map_err(|e| ContentError::Store(e.to_string()))?;

    let size = package.archive_bytes;
    let for_db = hash.clone();
    run_step(db, "packages.publish", move |db| {
        crate::db::with_typed_transaction(db.conn(), || {
            rows::record_published(db.conn(), &for_db, size)
        })
    })
    .await?;
    archive::discard_draft(&dirs.archives, &hash)?;
    required(db, &hash).await
}

/// Records a package whose archive is already in the content store, such as
/// one received with a course: checks it as import does and stores its
/// description. Returns the existing record when there is one. The caller
/// owns the archive's pin.
pub async fn register_stored(
    db: &impl PackageDb,
    node: &ContentNode,
    dirs: &PackageDirs,
    hash: &str,
) -> Result<Package, PackageError> {
    let hash = checked_hash(hash)?;
    if let Some(package) = get(db, &hash).await? {
        return Ok(package);
    }
    let exported = export_from_store(node, &dirs.cache, &hash).await?;
    let checked = blocking(move || {
        let checked = archive::check(&exported);
        let _ = std::fs::remove_file(&exported);
        checked
    })
    .await?;
    if checked.hash != hash {
        return Err(PackageError::Io(std::io::Error::other(format!(
            "the stored archive for {hash} does not match its hash"
        ))));
    }
    let package = Package {
        hash: checked.hash,
        standard: checked.description.standard,
        edition: checked.description.edition,
        title: checked.description.title.clone(),
        description: checked.description,
        archive_bytes: checked.size,
        location: ArchiveLocation::Store,
        imported_at: crate::clock::now_rfc3339(),
        last_opened_at: None,
    };
    run_step(db, "packages.register-stored", move |db| {
        rows::insert(db.conn(), &package)
    })
    .await
}

/// Where a package archive this device lacks comes from: the peers that
/// serve the course's content, by the archive's hash. On success the blob is
/// complete, and retained, in this device's content store.
pub trait ArchiveSource: Sync {
    fn fetch(&self, hash: &str) -> impl Future<Output = Result<(), PackageError>> + Send;
}

/// The app's content fetch path: the local store, then the peers that
/// provide the hash, BLAKE3-verified end to end.
impl ArchiveSource for crate::content_store::resolver::ContentResolver {
    async fn fetch(&self, hash: &str) -> Result<(), PackageError> {
        self.resolve(hash).await.map(|_| ()).map_err(|e| match e {
            crate::content_store::resolver::ResolveError::NotFound(_) => {
                PackageError::NotFound(hash.to_string())
            }
            other => PackageError::Content(ContentError::Store(other.to_string())),
        })
    }
}

/// The package with this hash for a learner: the record on this device, or,
/// when the hash is a package element of a published course on this device
/// and the archive is not here yet, the archive fetched from `source` by its
/// hash, checked as import checks it, recorded and pinned as the course's
/// content (evictable once no active enrolment needs it). `None` when the
/// hash belongs to no published course here: a hash the player asks for is
/// never fetched on its own say-so. A record whose archive was evicted from
/// the store, and whose files are not extracted, is fetched again.
pub async fn get_or_receive(
    db: &impl PackageDb,
    node: &ContentNode,
    dirs: &PackageDirs,
    source: &impl ArchiveSource,
    hash: &str,
) -> Result<Option<Package>, PackageError> {
    let hash = checked_hash(hash)?;
    let existing = get(db, &hash).await?;
    if let Some(package) = &existing {
        let evicted = package.location == ArchiveLocation::Store
            && !dirs.cache.join(&hash).is_dir()
            && !in_store(node, &hash).await?;
        if !evicted {
            return Ok(existing);
        }
    }
    let for_db = hash.clone();
    let published = run_step(db, "packages.receive.course", move |db| {
        rows::in_published_course(db.conn(), &for_db)
    })
    .await?;
    if !published {
        return Ok(existing);
    }
    if !in_store(node, &hash).await? {
        source.fetch(&hash).await?;
    }
    let package = register_stored(db, node, dirs, &hash).await?;
    let (for_db, size) = (hash.clone(), package.archive_bytes);
    run_step(db, "packages.receive.pin", move |db| {
        rows::record_received(db.conn(), &for_db, size)
    })
    .await?;
    Ok(Some(package))
}

/// The package hashes a course on this device carries, in course order.
pub async fn course_packages(
    db: &impl PackageDb,
    course_id: &str,
) -> Result<Vec<String>, PackageError> {
    let course_id = course_id.to_string();
    run_step(db, "packages.course-packages", move |db| {
        rows::course_packages(db.conn(), &course_id)
    })
    .await
}

/// Whether the store holds the whole blob.
async fn in_store(node: &ContentNode, hash: &str) -> Result<bool, PackageError> {
    let store = node
        .store()
        .await
        .map_err(|_| ContentError::NodeNotRunning)?;
    let status = store
        .status(content::parse_hash(hash)?)
        .await
        .map_err(|e| ContentError::Store(e.to_string()))?;
    Ok(matches!(status, BlobStatus::Complete { .. }))
}

/// Copies an archive out of the content store into a scratch file in the
/// cache directory, for one import check or extraction. The caller deletes it.
async fn export_from_store(
    node: &ContentNode,
    cache_dir: &Path,
    hash: &str,
) -> Result<PathBuf, PackageError> {
    let store = node
        .store()
        .await
        .map_err(|_| ContentError::NodeNotRunning)?;
    let parsed = content::parse_hash(hash)?;
    match store
        .status(parsed)
        .await
        .map_err(|e| ContentError::Store(e.to_string()))?
    {
        BlobStatus::Complete { .. } => {}
        BlobStatus::NotFound | BlobStatus::Partial { .. } => {
            return Err(ContentError::NotFound(hash.to_string()).into())
        }
    }
    let target = cache_dir.join(format!(
        "{}{hash}-{}.zip",
        cache::EXPORTED,
        uuid::Uuid::new_v4()
    ));
    if let Err(error) = store.export(parsed, &target).finish().await {
        let _ = std::fs::remove_file(&target);
        return Err(ContentError::Store(error.to_string()).into());
    }
    Ok(target)
}
