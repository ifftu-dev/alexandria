//! Standards packages: import a package archive from a file on disk, open a
//! package file as a draft course, look one up, and extract it for a launch.
//! The logic is in [`crate::packages`]; these wrappers find the profile's
//! directories and run database work on the executor.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use rusqlite::OptionalExtension;
use tauri::ipc::Channel;

use crate::commands::courses::{prepare_course_document_core, CourseSigner};
use crate::content_store::node::ContentNode;
use crate::crypto::wallet;
use crate::db::executor::DatabaseWorkload;
use crate::db::Database;
use crate::domain::package::{OpenedPackageCourse, Package, PackageExtractProgress};
use crate::enrollment;
use crate::packages::{self, PackageDb, PackageDirs};
use crate::profile::scope::ProfileState as State;
use crate::AppState;

/// Least time between two progress messages; the last one always goes.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

fn dirs(state: &State<'_, AppState>) -> Result<PackageDirs, String> {
    let dirs = PackageDirs::for_profile(&state.active_paths()?);
    dirs.ensure().map_err(|e| e.to_string())?;
    Ok(dirs)
}

use super::executor_db as db;

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
/// one package element, and enrols the learner in it locally so the player
/// can run it. Never announced or served. Needs the unlocked vault: the
/// local enrolment rests on a document signed with the learner's key.
#[tauri::command]
pub async fn package_open_file(
    state: State<'_, AppState>,
    path: String,
) -> Result<OpenedPackageCourse, String> {
    let dirs = dirs(&state)?;
    let keystore = state.keystore.lock().await;
    let mnemonic = keystore
        .as_ref()
        .ok_or("vault is locked — unlock first")?
        .retrieve_mnemonic()
        .map_err(|e| e.to_string())?;
    drop(keystore);
    let wallet = wallet::wallet_from_mnemonic(&mnemonic).map_err(|e| e.to_string())?;
    open_for_learning(
        &db(&state, DatabaseWorkload::Learner),
        &state.content_node,
        &dirs,
        CourseSigner {
            key: &wallet.signing_key,
            stake_address: &wallet.stake_address,
        },
        &PathBuf::from(path),
    )
    .await
}

/// Opens a package file as a draft course ([`packages::open_file`]) and
/// returns it with the learner's active enrolment in it, making one the
/// first time: the draft's document is signed and stored for this device
/// only (no catalog entry, no announcement), and the package's archive stays
/// a draft file the node cannot serve.
async fn open_for_learning(
    db: &impl PackageDb,
    node: &ContentNode,
    dirs: &PackageDirs,
    signer: CourseSigner<'_>,
    source: &Path,
) -> Result<OpenedPackageCourse, String> {
    let opened = packages::open_file(db, dirs, source)
        .await
        .map_err(|e| e.to_string())?;
    let course_id = opened.course_id.clone();
    let existing = db
        .run("packages.open-file.enrolment", move |db| {
            active_enrolment(db, &course_id)
        })
        .await?;
    let enrollment_id = match existing {
        Some(id) => id,
        None => {
            prepare_course_document_core(db, node, dirs, signer, &opened.course_id, false, &|| {
                true
            })
            .await?;
            let course_id = opened.course_id.clone();
            db.run("packages.open-file.enrol", move |db| {
                enrollment::enroll(db, &course_id)
                    .map(|enrolled| enrolled.id)
                    .map_err(|e| e.to_string())
            })
            .await?
        }
    };
    Ok(OpenedPackageCourse {
        opened,
        enrollment_id,
    })
}

fn active_enrolment(db: &Database, course_id: &str) -> Result<Option<String>, String> {
    db.conn()
        .query_row(
            "SELECT id FROM enrollments WHERE course_id = ?1 AND status = 'active' \
             ORDER BY enrolled_at DESC LIMIT 1",
            [course_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())
}

/// The stored package with this hash, if this profile knows it. A package
/// of a published course on this device that is not here yet is fetched
/// from the peers serving the course, by its hash, and recorded first
/// ([`packages::get_or_receive`]); `null` when it cannot be.
#[tauri::command]
pub async fn package_get(
    state: State<'_, AppState>,
    hash: String,
) -> Result<Option<Package>, String> {
    let db = db(&state, DatabaseWorkload::Learner);
    let resolver = state.resolver.lock().await.as_ref().cloned();
    let Some(resolver) = resolver else {
        return packages::get(&db, &hash).await.map_err(|e| e.to_string());
    };
    let dirs = dirs(&state)?;
    match packages::get_or_receive(&db, &state.content_node, &dirs, &resolver, &hash).await {
        Ok(package) => Ok(package),
        // No peer serves it right now: the player says it is not here yet.
        Err(packages::PackageError::NotFound(_)) => Ok(None),
        Err(error) => Err(error.to_string()),
    }
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

#[cfg(test)]
mod tests {
    //! Opening a package file to learn from, at the host seam: a migrated
    //! database, a running content node and the profile's package
    //! directories.

    use std::sync::Mutex;

    use ed25519_dalek::SigningKey;
    use tempfile::TempDir;

    use super::*;
    use crate::content_store::content;
    use crate::domain::package::PackageArchiveLocation;
    use crate::packages::attempts;
    use crate::packages::tests::induction_package;

    const LEARNER: &str = "stake_test1ulearner";

    struct Learner {
        db: Mutex<Database>,
        node: ContentNode,
        dirs: PackageDirs,
        key: SigningKey,
        root: TempDir,
    }

    impl Learner {
        async fn new() -> Self {
            let root = TempDir::new().expect("temp dir");
            let dirs = PackageDirs {
                archives: root.path().join("packages"),
                cache: root.path().join("packagecache"),
            };
            dirs.ensure().expect("package dirs");
            let node = ContentNode::new(&root.path().join("iroh"));
            node.start(None).await.expect("start node");
            let db = Database::open_in_memory().expect("in-memory db");
            db.run_migrations().expect("migrations");
            db.conn()
                .execute(
                    "INSERT INTO local_identity (id, stake_address, payment_address) \
                     VALUES (1, ?1, 'addr_test1q123')",
                    [LEARNER],
                )
                .expect("identity");
            Self {
                db: Mutex::new(db),
                node,
                dirs,
                key: SigningKey::from_bytes(&[9u8; 32]),
                root,
            }
        }

        async fn open(&self) -> OpenedPackageCourse {
            let source = self.root.path().join("induction.zip");
            std::fs::write(&source, induction_package()).expect("write package");
            open_for_learning(
                &self.db,
                &self.node,
                &self.dirs,
                CourseSigner {
                    key: &self.key,
                    stake_address: LEARNER,
                },
                &source,
            )
            .await
            .expect("open the package file")
        }

        fn count(&self, sql: &str) -> i64 {
            self.db
                .lock()
                .unwrap()
                .conn()
                .query_row(sql, [], |row| row.get(0))
                .unwrap()
        }
    }

    #[tokio::test]
    async fn a_package_file_opens_enrolled_so_the_player_can_start_an_attempt() {
        let learner = Learner::new().await;

        let opened = learner.open().await;

        let attempt = attempts::open(
            &learner.db,
            &opened.enrollment_id,
            &opened.opened.element_id,
        )
        .await
        .expect("an attempt opens under the local enrolment");
        assert_eq!(attempt.package_hash, opened.opened.package.hash);

        // Still a draft on this device: not in the catalog, its archive not
        // in the store the node serves from.
        assert_eq!(learner.count("SELECT COUNT(*) FROM catalog"), 0);
        assert!(!content::has(&learner.node, &opened.opened.package.hash)
            .await
            .unwrap());
        let stored = packages::get(&learner.db, &opened.opened.package.hash)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.location, PackageArchiveLocation::Draft);
        assert_eq!(
            learner.count("SELECT COUNT(*) FROM courses WHERE status = 'draft'"),
            1
        );
    }

    #[tokio::test]
    async fn opening_the_same_package_file_again_returns_the_same_enrolment() {
        let learner = Learner::new().await;
        let first = learner.open().await;

        let again = learner.open().await;

        assert_eq!(again.enrollment_id, first.enrollment_id);
        assert_eq!(again.opened.course_id, first.opened.course_id);
        assert_eq!(learner.count("SELECT COUNT(*) FROM enrollments"), 1);
    }
}
