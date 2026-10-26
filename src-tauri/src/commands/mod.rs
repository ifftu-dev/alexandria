//! Tauri commands: the adapter between the webview and the app's modules.
//!
//! Commands depend on the modules below them and never the other way round:
//! a module outside `commands` must not import from here (the command guard,
//! `scripts/check-tauri-commands.mjs`, fails if one does). Behaviour another
//! module needs belongs in the module named for it, with the command as a
//! thin wrapper over it. `lib.rs` is the one exception, because it registers
//! the commands and runs the background tasks.

use crate::db::executor::DatabaseWorkload;
use crate::profile::scope::ProfileState as State;
use crate::AppState;

pub mod adaptive;
pub mod aggregation;
pub mod assessment;
pub mod attestation;
pub mod auto_issuance;
pub mod catalog;
pub mod chapters;
pub mod classroom;
pub mod cli_install;
pub mod completion;
pub mod course_notes;
pub mod courses;
pub mod credentials;
pub mod demo_courses;
pub mod demo_resources;
pub mod dev_seed_reset;
pub mod dev_seeds;
pub mod diagnostics;
pub mod elements;
pub mod enrollment;
pub mod evidence;
pub mod goal_templates;
pub mod governance_genesis;
pub mod graph;
pub mod guardian;
pub mod health;
pub mod holder_pull;
pub mod holder_release;
pub mod identity;
pub mod import;
pub mod instructor;
pub mod integrity;
pub mod interview;
#[cfg(test)]
mod live_demo;
pub mod opinions;
pub mod personhood_lab;
pub mod personhood_receipts;
pub mod pinning;
pub mod plugins;
pub mod presentation;
pub mod profile;
pub mod reputation;
pub mod role_assessment;
pub mod sentinel_display;
pub mod sentinel_environment;
pub mod sentinel_evidence;
pub mod sentinel_gaze;
pub mod sentinel_holdout;
pub mod sentinel_hotkeys;
pub mod sentinel_ml;
pub mod sentinel_overlay;
pub mod sentinel_processes;
pub mod settings;
pub mod skill_bootstrap;
pub mod snapshot;
pub mod talent_index;
pub mod taxonomy;
pub mod updater;
pub mod username_registry;
pub mod users;

pub mod content;
pub mod p2p;
pub mod pairing;
pub mod storage;
pub mod sync;

#[cfg(any(desktop, target_os = "android"))]
pub mod tutoring;

#[cfg(target_os = "ios")]
pub mod tutoring_mobile;
#[cfg(target_os = "ios")]
pub use tutoring_mobile as tutoring;

#[cfg(not(any(desktop, target_os = "ios", target_os = "android")))]
pub mod tutoring_stubs;

pub mod ai_chat;
pub mod ai_signin;
pub mod studio;

pub mod studio_mcp;

pub mod decisions;

pub mod exchange;
pub mod hiring;
pub mod interview_panel;
pub mod organisations;

pub mod discussions;

/// Runs `f` on the active profile's database as learner work, under the
/// command's profile lease, so it is refused once the profile it started
/// for has been locked or switched. `label` names the work in the
/// executor's timing logs. Errors are the command's own `code: reason`
/// strings, returned unchanged.
pub(crate) async fn with_conn<T, F>(
    state: &State<'_, AppState>,
    label: &'static str,
    f: F,
) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(&rusqlite::Connection) -> Result<T, String> + Send + 'static,
{
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            label,
            move |db| f(db.conn()),
        )
        .await
}
