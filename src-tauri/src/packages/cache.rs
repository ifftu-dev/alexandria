//! The extracted-files cache: one directory per package hash under the
//! profile's package cache. Blocking; runs inside `spawn_blocking`.
//!
//! A directory named by a hash is always whole. Extraction writes into a
//! scratch sibling and renames it into place, and dropping renames the
//! directory aside before deleting it, so a crash at any point leaves either
//! a whole directory or a scratch name that [`clear_scratch`] removes.

use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};

use alexandria_studio::package::{extract_package, ExtractError, ExtractProgress};

use super::{PackageError, LIMITS};

/// Prefix of a directory being extracted.
const EXTRACTING: &str = ".extract-";
/// Prefix of a directory being deleted.
const DROPPING: &str = ".drop-";
/// Prefix of an archive copied out of the content store for one extraction.
pub(super) const EXPORTED: &str = ".archive-";
/// Prefix of a draft archive being imported.
pub(super) const IMPORTING: &str = ".import-";

pub(super) fn extracted_path(cache: &Path, hash: &str) -> PathBuf {
    cache.join(hash)
}

/// Extracts `archive` into the cache directory for `hash` unless it is
/// already there, and returns that directory.
pub(super) fn extract(
    cache: &Path,
    hash: &str,
    archive: &Path,
    progress: &mut dyn FnMut(&ExtractProgress<'_>),
) -> Result<PathBuf, PackageError> {
    let target = extracted_path(cache, hash);
    if target.is_dir() {
        return Ok(target);
    }
    let scratch = cache.join(format!("{EXTRACTING}{hash}-{}", uuid::Uuid::new_v4()));
    let extracted = File::open(archive)
        .map_err(ExtractError::Io)
        .and_then(|file| extract_package(file, &scratch, &LIMITS, progress));
    if let Err(error) = extracted {
        let _ = fs::remove_dir_all(&scratch);
        return Err(match error {
            ExtractError::Refused(refusal) => PackageError::Refused(refusal),
            ExtractError::Io(error) => PackageError::Io(error),
            ExtractError::TargetNotEmpty => PackageError::Io(io::Error::other(
                "the scratch directory for extraction is not empty",
            )),
        });
    }
    match fs::rename(&scratch, &target) {
        Ok(()) => Ok(target),
        // Another extraction of the same package finished first.
        Err(_) if target.is_dir() => {
            fs::remove_dir_all(&scratch)?;
            Ok(target)
        }
        Err(error) => {
            let _ = fs::remove_dir_all(&scratch);
            Err(error.into())
        }
    }
}

/// Moves the cache directory for `hash` out of the way, so nothing can find
/// it any more, and returns where it went. `None` when nothing was there.
pub(super) fn retire(cache: &Path, hash: &str) -> io::Result<Option<PathBuf>> {
    let target = extracted_path(cache, hash);
    let aside = cache.join(format!("{DROPPING}{hash}-{}", uuid::Uuid::new_v4()));
    match fs::rename(&target, &aside) {
        Ok(()) => Ok(Some(aside)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

/// Deletes every retired directory.
pub(super) fn empty_trash(cache: &Path) -> io::Result<()> {
    for entry in fs::read_dir(cache)? {
        let entry = entry?;
        if entry.file_name().to_string_lossy().starts_with(DROPPING) {
            remove(&entry.path())?;
        }
    }
    Ok(())
}

/// Retires what an interrupted extraction, export or import left behind.
/// Call only while nothing is extracting or importing: at profile start,
/// before commands are admitted.
pub(super) fn clear_scratch(cache: &Path, archives: &Path) -> io::Result<()> {
    for entry in fs::read_dir(cache)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with(EXTRACTING) || name.starts_with(EXPORTED) {
            let aside = cache.join(format!("{DROPPING}{}", uuid::Uuid::new_v4()));
            fs::rename(entry.path(), aside)?;
        }
    }
    for entry in fs::read_dir(archives)? {
        let entry = entry?;
        if entry.file_name().to_string_lossy().starts_with(IMPORTING) {
            fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}

/// Removes a file or a directory tree without following links.
fn remove(path: &Path) -> io::Result<()> {
    let kind = fs::symlink_metadata(path)?.file_type();
    if kind.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}
