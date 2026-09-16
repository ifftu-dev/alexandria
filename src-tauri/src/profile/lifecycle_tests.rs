use std::sync::Arc;

use tokio::sync::Mutex;

use crate::content_store::discovery::ContentDiscovery;
use crate::content_store::node::ContentNode;
use crate::crypto::keystore::Keystore;
use crate::profile::{Avatar, ProfileManager};
use crate::{ActiveProfile, AppState, ClassroomManager, TutoringManager};

pub(super) fn state_in(directory: &std::path::Path) -> Arc<AppState> {
    let db = Arc::new(std::sync::Mutex::new(None));
    Arc::new(AppState {
        app_data_dir: directory.to_path_buf(),
        profile_manager: Arc::new(ProfileManager::open(directory).expect("profile manager")),
        active: Arc::new(std::sync::RwLock::new(None)),
        profile_operations: super::operations::ProfileOperations::default(),
        tutoring: Arc::new(TutoringManager::new()),
        classroom: Arc::new(ClassroomManager::new()),
        studio: Arc::new(crate::commands::studio::StudioRuntime::default()),
        #[cfg(grader)]
        grader_runtime: Arc::new(
            crate::plugins::wasm_runtime::GraderRuntime::new().expect("grader"),
        ),
        last_activity: Arc::new(std::sync::Mutex::new(std::time::Instant::now())),
        ipc_limiter: Arc::new(std::sync::Mutex::new(
            crate::commands::ratelimit::IpcRateLimiter::new(),
        )),
        evidence_staging: Arc::new(crate::sentinel::evidence::EvidenceStaging::new()),
        db_executor: crate::db::executor::DatabaseExecutor::new(db.clone()),
        db,
        keystore: Arc::new(Mutex::new(None)),
        content_node: Arc::new(ContentNode::new(&directory.join("unused-content"))),
        resolver: Arc::new(Mutex::new(None)),
        discovery: Arc::new(ContentDiscovery::new()),
        p2p_node: Arc::new(Mutex::new(None)),
    })
}

#[tokio::test]
async fn profile_cleanup_clears_classroom_subscriptions() {
    let directory = tempfile::TempDir::new().expect("temporary profile directory");
    let state = state_in(directory.path());
    state
        .classroom
        .mark_subscribed("old-profile-classroom")
        .await;
    let cleanup_state = state.clone();
    state
        .profile_operations
        .lock(async move { cleanup_state.stop_active_profile().await })
        .await
        .expect("cleanup without an active content node");
    assert!(!state.classroom.is_subscribed("old-profile-classroom").await);
    state
        .stop_active_profile()
        .await
        .expect("idempotent cleanup");
}

#[tokio::test]
async fn profile_cleanup_purges_materialized_media_before_unlock_can_resume() {
    let directory = tempfile::TempDir::new().expect("temporary profile directory");
    let state = state_in(directory.path());
    let paths = state
        .profile_manager
        .create("Test learner", Avatar::default())
        .expect("profile");
    std::fs::write(paths.video_cache_dir.join("lesson.mp4"), b"private media")
        .expect("materialized media");
    let nested = paths.video_cache_dir.join("segments");
    std::fs::create_dir(&nested).expect("nested cache directory");
    std::fs::write(nested.join("segment.bin"), b"private segment").expect("cached segment");
    *state.active.write().expect("active profile lock") = Some(ActiveProfile {
        id: paths.id.clone(),
        paths: paths.clone(),
    });

    let cleanup_state = state.clone();
    state
        .profile_operations
        .lock(async move { cleanup_state.stop_active_profile().await })
        .await
        .expect("profile cleanup");

    assert!(paths.video_cache_dir.is_dir());
    assert_eq!(
        std::fs::read_dir(&paths.video_cache_dir)
            .expect("cache directory")
            .count(),
        0,
        "locked profiles must retain no asset-protocol media"
    );
}

#[tokio::test]
async fn failed_profile_start_reclaims_database_keystore_and_content_key() {
    let directory = tempfile::TempDir::new().expect("temporary profile directory");
    let state = state_in(directory.path());
    let paths = state
        .profile_manager
        .create("Test learner", Avatar::default())
        .expect("profile");
    let keystore = Keystore::create(&paths.vault_dir, "test-password-123").expect("vault");
    std::fs::create_dir_all(&paths.iroh_dir).expect("content directory");
    std::fs::write(
        paths.iroh_dir.join("node_secret.key"),
        b"invalid-key-fixture",
    )
    .expect("corrupt key fixture");
    let startup_state = state.clone();
    let cleanup_state = state.clone();
    let startup_paths = paths.clone();

    let result = state
        .profile_operations
        .activate(
            async move {
                startup_state
                    .start_active_profile(startup_paths, keystore)
                    .await
            },
            async move { cleanup_state.stop_active_profile().await },
        )
        .await;

    assert!(result
        .expect_err("invalid node key")
        .contains("key persistence"));
    assert!(state.db.lock().expect("database lock").is_none());
    assert!(state.keystore.lock().await.is_none());
    assert!(state.active_id().is_none());
    assert!(state.active_paths().is_err());
    assert!(state.resolver.lock().await.is_none());
    assert!(state.content_node.content_key().await.is_none());
    assert!(!state.content_node.is_running().await);
    assert!(!state.profile_operations.cleanup_required().await);
    assert!(
        paths.db_path.exists(),
        "rollback preserves the profile's database"
    );
    assert!(
        Keystore::exists(&paths.vault_dir),
        "rollback preserves the vault"
    );
}
