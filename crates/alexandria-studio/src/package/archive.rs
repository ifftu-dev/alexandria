//! Reading a package archive as something hostile.
//!
//! One rule decides everything here: nothing an archive says about itself is
//! believed. Entry names are checked before a byte is written, sizes are
//! counted as the data comes out of the decompressor, and the compression
//! ratio is taken against the length of the archive itself, which is the one
//! number its author cannot inflate.

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use zip::read::{ArchiveOffset, Config};
use zip::ZipArchive;

use super::{Limits, Refusal, RefusalCode};

/// Bytes the zip directory may cost to read before any entry is counted: the
/// end record with the longest comment the format allows, with room to spare.
const DIRECTORY_SLACK: u64 = 256 << 10;
/// Directory bytes allowed per permitted entry. A real record is a 46-byte
/// header plus the entry's path; this is several times a long path.
const DIRECTORY_BYTES_PER_ENTRY: u64 = 512;
/// How much extraction writes between two progress reports within one file.
const PROGRESS_STEP: u64 = 1 << 20;
const COPY_BUFFER: usize = 64 << 10;

/// An opened archive whose every entry name has been checked.
pub(crate) struct Archive<R> {
    zip: ZipArchive<Budgeted<R>>,
    len: u64,
    entries: Vec<Entry>,
    /// File path as the archive spells it, to its place in `entries`.
    exact: HashMap<String, usize>,
    /// Lower-cased file path to its place in `entries`.
    folded: HashMap<String, usize>,
}

struct Entry {
    /// Position in the zip directory.
    index: usize,
    /// Checked path: `/`-separated, relative, no `.` or `..` parts.
    path: String,
    is_dir: bool,
}

/// A file in an [`Archive`].
#[derive(Clone, Copy)]
pub(crate) struct FileRef(usize);

pub(crate) struct Measured {
    pub files: u64,
    pub bytes: u64,
}

impl<R: Read + Seek> Archive<R> {
    pub(crate) fn open(mut reader: R, limits: &Limits) -> Result<Self, Refusal> {
        let len = reader
            .seek(SeekFrom::End(0))
            .map_err(|error| not_an_archive(format!("the archive could not be read: {error}")))?;

        // The zip reader loads the whole directory before it can be asked how
        // many entries there are, and each one costs it memory. Bound what it
        // may read while doing so, so that a directory of millions of entries
        // is cut off long before it is held.
        let entry_allowance = (limits.max_entries as u64).saturating_mul(DIRECTORY_BYTES_PER_ENTRY);
        let budget = Rc::new(Cell::new(DIRECTORY_SLACK.saturating_add(entry_allowance)));
        let config = Config {
            archive_offset: ArchiveOffset::Known(0),
        };
        let reader = Budgeted {
            inner: reader,
            remaining: Rc::clone(&budget),
        };
        let zip = ZipArchive::with_config(config, reader).map_err(|error| {
            if budget.get() == 0 {
                too_many_entries(limits)
            } else {
                not_an_archive(format!("not a readable zip archive: {error}"))
            }
        })?;
        budget.set(u64::MAX);

        if zip.len() > limits.max_entries {
            return Err(too_many_entries(limits));
        }

        let mut entries = Vec::with_capacity(zip.len());
        let mut exact = HashMap::new();
        let mut folded = HashMap::new();
        let mut folders = HashSet::new();
        let mut declared = Allowance::new(limits, len);
        for index in 0..zip.len() {
            let entry = zip
                .by_index_data(index)
                .map_err(|error| not_an_archive(format!("entry {index} is unreadable: {error}")))?;
            let name = entry
                .name()
                .map_err(|error| not_an_archive(format!("entry {index} has no name: {error}")))?;
            let unsafe_entry = |why: &str| {
                Refusal::new(
                    RefusalCode::UnsafeEntryPath,
                    format!("entry {:?} {why}", name.as_ref()),
                )
            };

            match entry.unix_mode().map(|mode| mode & S_IFMT) {
                None | Some(0) | Some(S_IFREG) | Some(S_IFDIR) => {}
                Some(S_IFLNK) => return Err(unsafe_entry("is a link")),
                Some(_) => return Err(unsafe_entry("is not a regular file or folder")),
            }
            if entry.is_symlink() {
                return Err(unsafe_entry("is a link"));
            }
            let path = safe_path(&name).map_err(unsafe_entry)?;
            let is_dir = entry.is_dir() || name.ends_with(['/', '\\']);
            if entry.encrypted() {
                return Err(not_an_archive(format!(
                    "entry {:?} is encrypted",
                    name.as_ref()
                )));
            }
            if path.is_empty() {
                if is_dir {
                    continue;
                }
                return Err(unsafe_entry("has no name"));
            }

            // Two entries that would land on one file, on any of the
            // filesystems this runs on, are refused instead of letting the
            // second overwrite the first.
            let key = path.to_lowercase();
            let mut parent = key.as_str();
            while let Some((up, _)) = parent.rsplit_once('/') {
                folders.insert(up.to_owned());
                parent = up;
            }
            if is_dir {
                folders.insert(key);
            } else {
                if folded.insert(key, entries.len()).is_some() {
                    return Err(unsafe_entry("names the same file as another entry"));
                }
                exact.insert(path.clone(), entries.len());
                declared.spend(entry.size())?;
            }
            entries.push(Entry {
                index,
                path,
                is_dir,
            });
        }
        if let Some(clash) = folded.keys().find(|key| folders.contains(*key)) {
            return Err(Refusal::new(
                RefusalCode::UnsafeEntryPath,
                format!("{clash:?} is both a file and a folder"),
            ));
        }

        Ok(Self {
            zip,
            len,
            entries,
            exact,
            folded,
        })
    }

    /// The file at exactly this path.
    pub(crate) fn file(&self, path: &str) -> Option<FileRef> {
        self.exact.get(path).copied().map(FileRef)
    }

    /// The archive's own spelling of a file path: the path itself when it is
    /// there, otherwise the one file that differs from it only by case.
    /// Packages built on a case-blind filesystem routinely get this wrong.
    pub(crate) fn spelling(&self, path: &str) -> Option<&str> {
        let at = self
            .exact
            .get(path)
            .or_else(|| self.folded.get(&path.to_lowercase()))?;
        Some(&self.entries[*at].path)
    }

    pub(crate) fn file_paths(&self) -> impl Iterator<Item = &str> {
        self.entries
            .iter()
            .filter(|entry| !entry.is_dir)
            .map(|entry| entry.path.as_str())
    }

    /// One file's contents, refused if they run past `cap`.
    pub(crate) fn read(&mut self, file: FileRef, cap: u64) -> Result<Vec<u8>, Refusal> {
        let entry = &self.entries[file.0];
        let mut reader = self
            .zip
            .by_index(entry.index)
            .map_err(|error| unreadable(&entry.path, error))?;
        let mut bytes = Vec::new();
        (&mut reader)
            .take(cap.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|error| unreadable(&entry.path, error))?;
        if bytes.len() as u64 > cap {
            return Err(Refusal::new(
                RefusalCode::LimitExceeded,
                format!("{} is larger than {cap} bytes", entry.path),
            ));
        }
        Ok(bytes)
    }

    /// Decompresses every file and throws the bytes away, to learn what the
    /// archive really holds and that all of it can be read.
    pub(crate) fn measure(&mut self, limits: &Limits) -> Result<Measured, Refusal> {
        let mut allowance = Allowance::new(limits, self.len);
        let mut files = 0;
        for entry in self.entries.iter().filter(|entry| !entry.is_dir) {
            let outcome = pour(
                &mut self.zip,
                entry,
                &mut io::sink(),
                &mut allowance,
                |_| {},
            );
            match outcome {
                Ok(()) => files += 1,
                Err(ExtractError::Refused(refusal)) => return Err(refusal),
                // `io::sink` does not fail and nothing else is written here.
                Err(other) => return Err(not_an_archive(other.to_string())),
            }
        }
        Ok(Measured {
            files,
            bytes: allowance.used,
        })
    }

    fn write_to(
        &mut self,
        target_dir: &Path,
        limits: &Limits,
        progress: &mut dyn FnMut(&ExtractProgress<'_>),
    ) -> Result<ExtractSummary, ExtractError> {
        let mut allowance = Allowance::new(limits, self.len);
        let mut files = 0;
        for entry in &self.entries {
            // Every part of `entry.path` is a plain name, so this cannot
            // leave `target_dir`.
            let destination = entry
                .path
                .split('/')
                .fold(target_dir.to_path_buf(), |path, part| path.join(part));
            if entry.is_dir {
                fs::create_dir_all(&destination)?;
                continue;
            }
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent)?;
            }
            // `create_new` never follows a link and never overwrites.
            let mut out = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&destination)?;
            let mut reported = allowance.used;
            pour(
                &mut self.zip,
                entry,
                &mut out,
                &mut allowance,
                |bytes_done| {
                    if bytes_done - reported >= PROGRESS_STEP {
                        reported = bytes_done;
                        progress(&ExtractProgress {
                            path: &entry.path,
                            file_complete: false,
                            files_done: files,
                            bytes_done,
                        });
                    }
                },
            )?;
            out.flush()?;
            files += 1;
            progress(&ExtractProgress {
                path: &entry.path,
                file_complete: true,
                files_done: files,
                bytes_done: allowance.used,
            });
        }
        Ok(ExtractSummary {
            files,
            bytes: allowance.used,
        })
    }
}

/// Copies one entry out of the archive, charging every byte to `allowance`
/// as it appears and reporting the running total after each chunk.
///
/// The zip reader stops an entry that runs past its declared size, and the
/// declared sizes were charged when the archive was opened, so the charge
/// here should never be the one that refuses. It is made anyway: the limits
/// are this module's promise, and it does not rest on another crate's check.
fn pour<R: Read + Seek>(
    zip: &mut ZipArchive<Budgeted<R>>,
    entry: &Entry,
    out: &mut dyn Write,
    allowance: &mut Allowance,
    mut tick: impl FnMut(u64),
) -> Result<(), ExtractError> {
    let mut reader = zip
        .by_index(entry.index)
        .map_err(|error| unreadable(&entry.path, error))?;
    let mut buffer = vec![0u8; COPY_BUFFER];
    loop {
        let read = match reader.read(&mut buffer) {
            Ok(0) => return Ok(()),
            Ok(read) => read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(unreadable(&entry.path, error).into()),
        };
        allowance.spend(read as u64)?;
        out.write_all(&buffer[..read])?;
        tick(allowance.used);
    }
}

/// Running total of decompressed bytes against the two size limits.
struct Allowance {
    used: u64,
    max_bytes: u64,
    ratio: u64,
    max_by_ratio: u64,
}

impl Allowance {
    fn new(limits: &Limits, archive_len: u64) -> Self {
        Self {
            used: 0,
            max_bytes: limits.max_uncompressed_bytes,
            ratio: limits.max_compression_ratio,
            max_by_ratio: limits.max_compression_ratio.saturating_mul(archive_len),
        }
    }

    fn spend(&mut self, bytes: u64) -> Result<(), Refusal> {
        self.used = self.used.saturating_add(bytes);
        if self.used > self.max_bytes {
            return Err(Refusal::new(
                RefusalCode::LimitExceeded,
                format!(
                    "the package holds more than {} bytes once decompressed",
                    self.max_bytes
                ),
            ));
        }
        if self.used > self.max_by_ratio {
            return Err(Refusal::new(
                RefusalCode::LimitExceeded,
                format!(
                    "the package decompresses to more than {} times its own size",
                    self.ratio
                ),
            ));
        }
        Ok(())
    }
}

/// A reader that stops after a set number of bytes, shared with whoever
/// opened it so the stop can be lifted.
struct Budgeted<R> {
    inner: R,
    remaining: Rc<Cell<u64>>,
}

impl<R: Read> Read for Budgeted<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let remaining = self.remaining.get();
        if remaining == 0 {
            return Err(io::Error::other("the zip directory is too large"));
        }
        let read = self.inner.read(buf)?;
        self.remaining.set(remaining.saturating_sub(read as u64));
        Ok(read)
    }
}

impl<R: Seek> Seek for Budgeted<R> {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.inner.seek(position)
    }
}

/// The longest single file or folder name the filesystems this runs on agree
/// to hold. Refused at import so that it is not found out during extraction.
const MAX_NAME_BYTES: usize = 255;

const S_IFMT: u32 = 0o170000;
const S_IFREG: u32 = 0o100000;
const S_IFDIR: u32 = 0o040000;
const S_IFLNK: u32 = 0o120000;

/// Turns an entry name into a path that can only land inside the directory
/// it is joined to, or says why it cannot be one.
///
/// Backslashes count as separators: archives written by older Windows tools
/// use them, and treating them as part of a name would let `..\` through on
/// Windows. Empty and `.` parts are dropped.
fn safe_path(name: &str) -> Result<String, &'static str> {
    if name.chars().any(char::is_control) {
        return Err("has a control character in its name");
    }
    let name = name.replace('\\', "/");
    if name.starts_with('/') {
        return Err("has an absolute path");
    }
    let bytes = name.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return Err("has an absolute path");
    }
    let mut parts = Vec::new();
    for part in name.split('/') {
        match part {
            "" | "." => {}
            ".." => return Err("climbs out of the package with `..`"),
            part if part.len() > MAX_NAME_BYTES => {
                return Err("has a name too long for a filesystem to hold")
            }
            part => parts.push(part),
        }
    }
    Ok(parts.join("/"))
}

fn not_an_archive(detail: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::NotAnArchive, detail)
}

fn unreadable(path: &str, error: impl std::fmt::Display) -> Refusal {
    not_an_archive(format!("{path} cannot be read from the archive: {error}"))
}

fn too_many_entries(limits: &Limits) -> Refusal {
    Refusal::new(
        RefusalCode::LimitExceeded,
        format!("the archive has more than {} entries", limits.max_entries),
    )
}

/// Reported while [`extract_package`] works: after every file, and about
/// once a mebibyte inside a large one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExtractProgress<'a> {
    /// The file being written, as a path inside the archive.
    pub path: &'a str,
    /// True once `path` is whole on disk.
    pub file_complete: bool,
    /// Files finished so far.
    pub files_done: u64,
    /// Bytes written so far, across all files.
    pub bytes_done: u64,
}

/// What [`extract_package`] wrote.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExtractSummary {
    pub files: u64,
    pub bytes: u64,
}

/// Why [`extract_package`] stopped.
#[derive(Debug, thiserror::Error)]
pub enum ExtractError {
    /// The archive itself is at fault. Same codes as import.
    #[error(transparent)]
    Refused(#[from] Refusal),
    /// The directory to extract into already holds something.
    #[error("target_not_empty: the directory to extract into is not empty")]
    TargetNotEmpty,
    /// The disk is at fault.
    #[error("io_error: {0}")]
    Io(#[from] io::Error),
}

/// Writes a package archive's files under `target_dir`.
///
/// `target_dir` must be missing or empty; it is created if missing. Each file
/// lands at its path inside the archive, which is the path a
/// [`Launch`](super::Launch) names. Entry names are all checked before the
/// first file is created, so an archive with an absolute path, a `..`, or a
/// link writes nothing. Links are never created, and the permission bits an
/// archive carries are ignored.
///
/// The same [`Limits`] as import apply, counted as bytes are written. On any
/// error everything written so far is removed and `target_dir` is left empty.
/// The files are not synced to disk and a crash can leave a partial tree:
/// extract into a scratch directory and rename it into place.
///
/// Files are written in the order the archive lists them. `progress` is
/// called from the calling thread; run this off the interface thread.
pub fn extract_package<R: Read + Seek>(
    archive: R,
    target_dir: &Path,
    limits: &Limits,
    progress: &mut dyn FnMut(&ExtractProgress<'_>),
) -> Result<ExtractSummary, ExtractError> {
    let mut archive = Archive::open(archive, limits)?;

    fs::create_dir_all(target_dir)?;
    if fs::read_dir(target_dir)?.next().is_some() {
        return Err(ExtractError::TargetNotEmpty);
    }

    let outcome = archive.write_to(target_dir, limits, progress);
    if outcome.is_err() {
        empty_dir(target_dir);
    }
    outcome
}

/// Best-effort removal of everything inside `dir`. The failure that led here
/// is the one worth reporting, so errors from the clean-up are dropped.
fn empty_dir(dir: &Path) {
    let Ok(children) = fs::read_dir(dir) else {
        return;
    };
    for child in children.flatten() {
        let path: PathBuf = child.path();
        let is_dir = child.file_type().is_ok_and(|kind| kind.is_dir());
        let _ = if is_dir {
            fs::remove_dir_all(&path)
        } else {
            fs::remove_file(&path)
        };
    }
}
