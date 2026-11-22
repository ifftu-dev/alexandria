//! Package launches: bind a package's extracted files to an unguessable
//! prefix under the package scheme, and retire it. The logic is in
//! [`crate::package_serving`].

use std::time::{Duration, Instant};

use tauri::ipc::Channel;

use crate::db::executor::DatabaseWorkload;
use crate::domain::package::PackageExtractProgress;
use crate::domain::package_launch::OpenedPackageLaunch;
use crate::package_serving;
use crate::packages::{ExecutorDb, PackageDirs};
use crate::profile::scope::ProfileState as State;
use crate::AppState;

/// Least time between two progress messages; the last one always goes.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

/// Opens a launch of the package with this hash, extracting it first if
/// needed and reporting progress on `on_progress`. The returned `base_url`
/// serves the package's files until [`package_launch_close`].
#[tauri::command]
pub async fn package_launch_open(
    state: State<'_, AppState>,
    hash: String,
    on_progress: Channel<PackageExtractProgress>,
) -> Result<OpenedPackageLaunch, String> {
    let dirs = PackageDirs::for_profile(&state.active_paths()?);
    dirs.ensure().map_err(|e| e.to_string())?;
    let mut last_sent: Option<Instant> = None;
    let progress = move |report: PackageExtractProgress| {
        let finished = report.files_done == report.files_total;
        if finished || last_sent.is_none_or(|at| at.elapsed() >= PROGRESS_INTERVAL) {
            last_sent = Some(Instant::now());
            let _ = on_progress.send(report);
        }
    };
    let db = ExecutorDb {
        executor: &state.db_executor,
        lease: state.profile_lease(),
        workload: DatabaseWorkload::Learner,
    };
    package_serving::open_launch(
        &state.package_launches,
        &db,
        &state.content_node,
        &dirs,
        &hash,
        progress,
    )
    .await
    .map_err(|e| e.to_string())
}

/// Closes a launch: nothing under its prefix is served afterwards. Returns
/// whether the launch was open.
#[tauri::command]
pub async fn package_launch_close(
    state: State<'_, AppState>,
    prefix: String,
) -> Result<bool, String> {
    Ok(state.package_launches.close(&prefix))
}
