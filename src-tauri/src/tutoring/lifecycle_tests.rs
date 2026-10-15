use super::TutoringManager;
use std::time::Duration;
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

#[tokio::test]
async fn leaving_returns_while_a_blocked_native_task_finishes_in_the_background() {
    let manager = TutoringManager::new();
    manager.tasks.open().expect("startup admission");
    let (release, released) = std::sync::mpsc::channel::<()>();
    let (started, running) = oneshot::channel::<()>();
    let tasks = manager.tasks.clone();
    let native = tokio::spawn(async move {
        // Stands in for a camera open stuck in native code: it cannot be
        // aborted, only waited for.
        let _ = tasks
            .run_blocking(move || {
                let _ = started.send(());
                let _ = released.recv();
            })
            .await;
    });
    running.await.expect("native work started");

    // Leaving does not wait for it. (Real clock: a paused one never
    // advances while blocking work runs.)
    tokio::time::timeout(Duration::from_secs(10), manager.shutdown())
        .await
        .expect("leave returns while native work is stuck")
        .expect("leave");
    assert!(!manager.tasks.is_open());
    // No second room starts while the first one's camera may still be held.
    assert!(manager.tasks.open().is_err());

    release.send(()).expect("finish native work");
    native.await.expect("native task");
    for _ in 0..100 {
        if manager.tasks.open().is_ok() {
            manager.shutdown().await.expect("finish fixture");
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("cleanup never finished after the native work returned");
}
