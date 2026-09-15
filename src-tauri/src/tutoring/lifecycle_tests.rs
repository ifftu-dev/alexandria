use super::TutoringManager;
use tokio::sync::oneshot;

#[tokio::test]
async fn shutdown_joins_partial_startup_tasks_without_a_published_room() {
    let manager = TutoringManager::new();
    manager.tasks.open().expect("startup admission");
    let (owner, mut released) = oneshot::channel::<()>();
    manager.tasks.spawn(async move {
        let _owner = owner;
        std::future::pending::<()>().await;
    });
    assert!(manager.inner.lock().await.is_none());
    manager.shutdown().await.expect("partial startup cleanup");
    assert!(matches!(
        released.try_recv(),
        Err(oneshot::error::TryRecvError::Closed)
    ));
    assert!(!manager.tasks.is_open());
    assert!(!manager.is_active().await);
    manager.shutdown().await.expect("idempotent cleanup");
}

#[tokio::test]
async fn interrupted_shutdown_still_blocks_task_admission_until_retry() {
    let manager = TutoringManager::new();
    manager.tasks.open().expect("startup admission");
    let (owner, released) = oneshot::channel::<()>();
    manager.tasks.spawn(async move {
        let _owner = owner;
        std::future::pending::<()>().await;
    });
    {
        let mut cleanup = Box::pin(manager.shutdown());
        assert!(futures::poll!(&mut cleanup).is_pending());
    }
    assert!(!manager.tasks.is_open());
    assert!(manager.tasks.open().is_err());
    manager.shutdown().await.expect("retry cleanup");
    assert!(released.await.is_err());
    manager.tasks.open().expect("next room may start");
    manager.shutdown().await.expect("finish fixture");
}
