//! Standards packages: import a package archive from a file on disk, open a
//! package file as a draft course, look one up, and extract it for a launch.
//! The logic is in [`crate::packages`]; these wrappers find the profile's
//! directories and run database work on the executor.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use tauri::ipc::Channel;

use crate::db::executor::DatabaseWorkload;
use crate::domain::package::{OpenedPackageFile, Package, PackageExtractProgress};
use crate::packages::{self, ExecutorDb, PackageDirs};
use crate::profile::scope::ProfileState as State;
use crate::AppState;

/// Least time between two progress messages; the last one always goes.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

fn dirs(state: &State<'_, AppState>) -> Result<PackageDirs, String> {
    let dirs = PackageDirs::for_profile(&state.active_paths()?);
    dirs.ensure().map_err(|e| e.to_string())?;
    Ok(dirs)
}

fn db<'a>(state: &'a State<'_, AppState>, workload: DatabaseWorkload) -> ExecutorDb<'a> {
    ExecutorDb {
        executor: &state.db_executor,
        lease: state.profile_lease(),
        workload,
    }
}

/// Imports the package archive at `path`, a file on this device, without
/// carrying its bytes over the bridge. Returns the stored package; a refused
/// archive fails with `<refusal code>: <detail>`.
#[tauri::command]
pub async fn package_import(state: State<'_, AppState>, path: String) -> Result<Package, String> {
    let dirs = dirs(&state)?;
    packages::import(
        &db(&state, DatabaseWorkload::Instructor),
        &dirs,
        &PathBuf::from(path),
    )
    .await
    .map_err(|e| e.to_string())
}

/// Opens the package file at `path` as a draft course on this device with
/// one package element. Never announced or served.
#[tauri::command]
pub async fn package_open_file(
    state: State<'_, AppState>,
    path: String,
) -> Result<OpenedPackageFile, String> {
    let dirs = dirs(&state)?;
    packages::open_file(
        &db(&state, DatabaseWorkload::Learner),
        &dirs,
        &PathBuf::from(path),
    )
    .await
    .map_err(|e| e.to_string())
}

/// The stored package with this hash, if this profile knows it.
#[tauri::command]
pub async fn package_get(
    state: State<'_, AppState>,
    hash: String,
) -> Result<Option<Package>, String> {
    packages::get(&db(&state, DatabaseWorkload::Learner), &hash)
        .await
        .map_err(|e| e.to_string())
}

/// Makes sure a package's files are extracted, reporting progress on
/// `on_progress` while extraction runs off the interface thread.
#[tauri::command]
pub async fn package_extract(
    state: State<'_, AppState>,
    hash: String,
    on_progress: Channel<PackageExtractProgress>,
) -> Result<Package, String> {
    let dirs = dirs(&state)?;
    let mut last_sent: Option<Instant> = None;
    let progress = move |report: PackageExtractProgress| {
        let finished = report.files_done == report.files_total;
        if finished || last_sent.is_none_or(|at| at.elapsed() >= PROGRESS_INTERVAL) {
            last_sent = Some(Instant::now());
            let _ = on_progress.send(report);
        }
    };
    packages::ensure_extracted(
        &db(&state, DatabaseWorkload::Learner),
        &state.content_node,
        &dirs,
        &hash,
        progress,
    )
    .await
    .map(|extracted| extracted.package)
    .map_err(|e| e.to_string())
}
