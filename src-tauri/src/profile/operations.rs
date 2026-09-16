use std::future::Future;
use std::sync::Arc;

use tokio::sync::Mutex;
use tokio::task::JoinSet;

#[derive(Clone, Default)]
pub(crate) struct ProfileOperations {
    gate: Arc<Mutex<Phase>>,
    jobs: Arc<Mutex<JoinSet<()>>>,
    admission: super::scope::Admission,
}

#[derive(Default, PartialEq, Eq, Debug)]
enum Phase {
    #[default]
    Locked,
    Active,
    CleanupRequired,
}

impl ProfileOperations {
    pub(crate) fn session(&self) -> Option<String> {
        self.admission.session()
    }

    pub(crate) fn admit(&self, session: &str) -> Result<super::scope::ProfileLease, String> {
        self.admission.admit(session)
    }

    pub(crate) async fn cleanup_required(&self) -> bool {
        *self.gate.lock().await == Phase::CleanupRequired
    }

    pub(crate) async fn activate<T>(
        &self,
        startup: impl Future<Output = Result<T, String>> + Send + 'static,
        cleanup: impl Future<Output = Result<(), String>> + Send + 'static,
    ) -> Result<T, String>
    where
        T: Send + 'static,
    {
        let gate = self.gate.clone();
        let admission = self.admission.clone();
        let requested_revision = admission.revision();
        tokio::spawn(async move {
            let mut phase = gate.lock().await;
            if *phase != Phase::Locked {
                return Err("finish locking before unlocking a profile".to_string());
            }
            // Fail closed even if startup unwinds. Publish Active only after
            // the entire command (including identity writes) has succeeded.
            let ticket = admission
                .begin_start(&requested_revision)
                .ok_or("profile activation was cancelled by locking")?;
            *phase = Phase::CleanupRequired;
            admission.drain().await;
            let result = startup.await.and_then(|value| {
                if admission.open(&ticket) {
                    Ok(value)
                } else {
                    Err("profile activation was cancelled by locking".to_string())
                }
            });
            match result {
                Ok(value) => {
                    *phase = Phase::Active;
                    Ok(value)
                }
                Err(start_error) => match cleanup.await {
                    Ok(()) => {
                        *phase = Phase::Locked;
                        Err(start_error)
                    }
                    Err(cleanup_error) => Err(format!(
                        "{start_error}; profile cleanup also failed: {cleanup_error}"
                    )),
                },
            }
        })
        .await
        .map_err(|e| format!("profile activation failed: {e}"))?
    }

    pub(crate) async fn lock(
        &self,
        cleanup: impl Future<Output = Result<(), String>> + Send + 'static,
    ) -> Result<(), String> {
        self.admission.close();
        let gate = self.gate.clone();
        let admission = self.admission.clone();
        tokio::spawn(async move {
            let mut phase = gate.lock().await;
            *phase = Phase::CleanupRequired;
            admission.close();
            admission.drain().await;
            cleanup.await?;
            *phase = Phase::Locked;
            Ok(())
        })
        .await
        .map_err(|e| format!("profile lock failed: {e}"))?
    }

    pub(crate) async fn spawn_job(&self, job: impl Future<Output = ()> + Send + 'static) {
        self.jobs.lock().await.spawn(job);
    }

    pub(crate) async fn stop_jobs(&self) {
        let mut jobs = self.jobs.lock().await;
        jobs.abort_all();
        // join_next is cancellation-safe: an interrupted cleanup keeps all
        // unjoined handles in the set for the next cleanup attempt.
        while let Some(result) = jobs.join_next().await {
            if let Err(e) = result {
                if !e.is_cancelled() {
                    log::warn!("profile background task failed: {e}");
                }
            }
        }
    }

    pub(crate) async fn run<T>(
        &self,
        operation: impl Future<Output = Result<T, String>> + Send + 'static,
    ) -> Result<T, String>
    where
        T: Send + 'static,
    {
        let gate = self.gate.clone();
        // The owned task, not the IPC caller, holds the transition gate. A
        // cancelled caller must not detach spawn_blocking vault work from the
        // operation that waits for it and finishes using its resources.
        tokio::spawn(async move {
            let _transition = gate.lock().await;
            if *_transition == Phase::CleanupRequired {
                return Err("finish profile cleanup before modifying profiles".to_string());
            }
            operation.await
        })
        .await
        .map_err(|e| format!("profile operation failed: {e}"))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::oneshot;

    #[tokio::test]
    async fn locking_during_startup_cancels_publication_and_rolls_back() {
        let operations = ProfileOperations::default();
        let (started, ready) = oneshot::channel();
        let (release, released) = oneshot::channel();
        let (rolled_back, rollback) = oneshot::channel();
        let mut activation = Box::pin(operations.activate(
            async move {
                started.send(()).expect("startup signal");
                released.await.expect("startup release");
                Ok(())
            },
            async move {
                rolled_back.send(()).expect("rollback signal");
                Ok(())
            },
        ));
        assert!(futures::poll!(&mut activation).is_pending());
        ready.await.expect("startup running");
        let mut lock = Box::pin(operations.lock(async { Ok(()) }));
        assert!(futures::poll!(&mut lock).is_pending());
        assert!(operations.session().is_none());
        release.send(()).expect("finish startup");
        assert!(activation
            .await
            .expect_err("lock cancels startup")
            .contains("cancelled by locking"));
        rollback.await.expect("startup rolled back");
        lock.await.expect("lock completes");
        assert!(operations.session().is_none());
        operations
            .activate(async { Ok(()) }, async { Ok(()) })
            .await
            .expect("next activation");
        assert!(operations.session().is_some());
    }

    #[tokio::test]
    async fn locking_invalidates_activation_waiting_for_the_transition_gate() {
        let operations = ProfileOperations::default();
        let gate = operations.gate.lock().await;
        let (started, mut startup) = oneshot::channel();
        let mut activation = Box::pin(operations.activate(
            async move {
                started.send(()).expect("startup signal");
                Ok(())
            },
            async { Ok(()) },
        ));
        assert!(futures::poll!(&mut activation).is_pending());
        let mut lock = Box::pin(operations.lock(async { Ok(()) }));
        assert!(futures::poll!(&mut lock).is_pending());
        drop(gate);
        assert!(activation.await.is_err());
        lock.await.expect("lock completes");
        assert!(matches!(
            startup.try_recv(),
            Err(oneshot::error::TryRecvError::Closed)
        ));
        assert!(operations.session().is_none());
    }

    #[tokio::test]
    async fn failed_startup_rolls_back_before_another_activation() {
        let operations = ProfileOperations::default();
        let resources = Arc::new(Mutex::new(Vec::new()));
        let started_resources = resources.clone();
        let cleanup_resources = resources.clone();
        let result = operations
            .activate(
                async move {
                    started_resources.lock().await.push("partial-profile");
                    Err::<(), _>("identity write failed".to_string())
                },
                async move {
                    cleanup_resources.lock().await.clear();
                    Ok(())
                },
            )
            .await;
        assert_eq!(result, Err("identity write failed".to_string()));
        assert!(resources.lock().await.is_empty());
        assert!(!operations.cleanup_required().await);
        assert_eq!(
            operations
                .activate(async { Ok("next") }, async { Ok(()) })
                .await,
            Ok("next")
        );
    }

    #[tokio::test]
    async fn failed_rollback_blocks_activation_and_deletion_until_lock_succeeds() {
        let operations = ProfileOperations::default();
        assert!(operations
            .activate(async { Err::<(), _>("startup".to_string()) }, async {
                Err("cleanup".to_string())
            })
            .await
            .expect_err("rollback failed")
            .contains("cleanup"));
        assert!(operations.cleanup_required().await);
        let polled = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let started = polled.clone();
        assert!(operations
            .activate(
                async move {
                    started.store(true, std::sync::atomic::Ordering::SeqCst);
                    Ok(())
                },
                async { Ok(()) }
            )
            .await
            .is_err());
        assert!(!polled.load(std::sync::atomic::Ordering::SeqCst));
        assert!(operations.run(async { Ok("delete") }).await.is_err());
        operations
            .lock(async { Ok(()) })
            .await
            .expect("retry cleanup");
        assert!(!operations.cleanup_required().await);
        operations
            .activate(async { Ok(()) }, async { Ok(()) })
            .await
            .expect("next activation");
    }

    #[tokio::test]
    async fn active_profile_cannot_be_replaced_without_successful_lock() {
        let operations = ProfileOperations::default();
        operations
            .activate(async { Ok(()) }, async { Ok(()) })
            .await
            .expect("activate");
        assert!(operations
            .activate(async { Ok(()) }, async { Ok(()) })
            .await
            .is_err());
        assert!(operations
            .lock(async { Err("shutdown".to_string()) })
            .await
            .is_err());
        assert!(operations.cleanup_required().await);
        assert!(operations
            .activate(async { Ok(()) }, async { Ok(()) })
            .await
            .is_err());
        operations.lock(async { Ok(()) }).await.expect("retry lock");
        operations
            .activate(async { Ok(()) }, async { Ok(()) })
            .await
            .expect("replace");
    }

    #[tokio::test]
    async fn startup_panic_leaves_cleanup_required() {
        let operations = ProfileOperations::default();
        let error: Result<(), String> = operations
            .activate(
                async {
                    std::panic::panic_any("startup fixture panic");
                },
                async { Ok(()) },
            )
            .await;
        assert!(error.is_err());
        assert!(operations.cleanup_required().await);
        operations
            .lock(async { Ok(()) })
            .await
            .expect("cleanup after panic");
    }

    #[tokio::test]
    async fn caller_cancellation_keeps_entire_operation_serialized() {
        let operations = ProfileOperations::default();
        let events = Arc::new(Mutex::new(Vec::new()));
        let (started, ready) = oneshot::channel();
        let (release, released) = oneshot::channel();
        let first_operations = operations.clone();
        let first_events = events.clone();
        let caller = tokio::spawn(async move {
            first_operations
                .run(async move {
                    first_events.lock().await.push("start-a");
                    tokio::task::spawn_blocking(move || {
                        started.send(()).expect("signal start");
                        released.blocking_recv().expect("release operation");
                    })
                    .await
                    .expect("blocking vault-work fixture");
                    first_events.lock().await.push("finish-a");
                    Ok(())
                })
                .await
        });
        ready.await.expect("operation started");
        caller.abort();
        assert!(caller.await.expect_err("caller cancelled").is_cancelled());
        assert!(
            operations.gate.try_lock().is_err(),
            "operation still owns gate"
        );

        let second_events = events.clone();
        let second = tokio::spawn(async move {
            operations
                .run(async move {
                    second_events.lock().await.push("start-b");
                    Ok(())
                })
                .await
        });
        assert_eq!(*events.lock().await, ["start-a"]);
        release.send(()).expect("release first operation");
        second
            .await
            .expect("second task")
            .expect("second operation");
        assert_eq!(*events.lock().await, ["start-a", "finish-a", "start-b"]);
    }

    #[tokio::test]
    async fn operation_error_is_preserved_and_cleanup_can_run_next() {
        let operations = ProfileOperations::default();
        assert_eq!(
            operations
                .run(async { Err::<(), _>("startup failed".to_string()) })
                .await,
            Err("startup failed".to_string())
        );
        assert_eq!(operations.run(async { Ok("cleanup") }).await, Ok("cleanup"));
    }

    #[tokio::test]
    async fn jobs_are_joined_before_resources_can_be_reused() {
        let operations = ProfileOperations::default();
        let (owner, mut released) = oneshot::channel::<()>();
        operations
            .spawn_job(async move {
                let _owner = owner;
                std::future::pending::<()>().await;
            })
            .await;

        operations.stop_jobs().await;

        assert!(matches!(
            released.try_recv(),
            Err(oneshot::error::TryRecvError::Closed)
        ));
        assert!(operations.jobs.lock().await.is_empty());
        operations.stop_jobs().await;
    }

    #[tokio::test]
    async fn interrupted_job_cleanup_keeps_unjoined_handles() {
        let operations = ProfileOperations::default();
        operations.spawn_job(std::future::pending::<()>()).await;
        {
            let mut cleanup = Box::pin(operations.stop_jobs());
            assert!(futures::poll!(&mut cleanup).is_pending());
        }
        assert_eq!(operations.jobs.lock().await.len(), 1);

        operations.stop_jobs().await;

        assert!(operations.jobs.lock().await.is_empty());
    }
}
