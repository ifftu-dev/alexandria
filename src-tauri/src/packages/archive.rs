//! Blocking file work on archives: taking a copy in, hashing it, importing it.
//! Everything here runs inside `spawn_blocking`.

use std::fs::{self, File};
use std::io::{self, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use alexandria_studio::package::{import_package, PackageDescription};

use super::cache::IMPORTING;
use super::{PackageError, LIMITS};

/// An archive whose copy has been hashed and imported.
pub(super) struct Checked {
    pub hash: String,
    pub size: u64,
    pub description: PackageDescription,
}

/// Copies `source` into `archives` and keeps it as `<hash>.zip`, but only if
/// import accepts the copy. The copy is what gets hashed and imported, so
/// the kept bytes are the ones that were checked even if `source` changes
/// meanwhile. The source is read once, as a stream.
pub(super) fn keep_draft(archives: &Path, source: &Path) -> Result<Checked, PackageError> {
    let scratch = archives.join(format!("{IMPORTING}{}.zip", uuid::Uuid::new_v4()));
    let outcome = copy_check_and_keep(archives, source, &scratch);
    if outcome.is_err() {
        let _ = fs::remove_file(&scratch);
    }
    outcome
}

fn copy_check_and_keep(
    archives: &Path,
    source: &Path,
    scratch: &Path,
) -> Result<Checked, PackageError> {
    fs::copy(source, scratch)?;
    let checked = check(scratch)?;
    let kept = draft_path(archives, &checked.hash);
    if kept.exists() {
        fs::remove_file(scratch)?;
    } else {
        fs::rename(scratch, &kept)?;
    }
    Ok(checked)
}

/// Hashes and imports an archive on disk.
pub(super) fn check(path: &Path) -> Result<Checked, PackageError> {
    let mut file = File::open(path)?;
    let mut hasher = blake3::Hasher::new();
    hasher.update_reader(&mut file)?;
    let size = file.stream_position()?;
    file.seek(SeekFrom::Start(0))?;
    let description = import_package(&mut file, &LIMITS).map_err(PackageError::Refused)?;
    Ok(Checked {
        hash: hasher.finalize().to_hex().to_string(),
        size,
        description,
    })
}

pub(super) fn draft_path(archives: &Path, hash: &str) -> PathBuf {
    archives.join(format!("{hash}.zip"))
}

/// Removes a draft archive; a missing one is not an error.
pub(super) fn discard_draft(archives: &Path, hash: &str) -> io::Result<()> {
    match fs::remove_file(draft_path(archives, hash)) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}
