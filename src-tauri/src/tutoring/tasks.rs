use std::future::{poll_fn, Future};
use std::sync::Mutex;

use tokio::task::{AbortHandle, JoinSet};

#[derive(Default)]
struct State {
    accepting: bool,
    tasks: JoinSet<()>,
}

#[derive(Default)]
pub(super) struct SessionTasks(Mutex<State>);

pub(super) struct TaskHandle(Option<AbortHandle>);

pub(super) struct StartupGuard<'a> {
    tasks: &'a SessionTasks,
    committed: bool,
}

impl StartupGuard<'_> {
    pub(super) fn commit(&mut self) {
        self.committed = true;
    }
}

impl Drop for StartupGuard<'_> {
    fn drop(&mut self) {
        if !self.committed {
            self.tasks.close();
        }
    }
}

impl TaskHandle {
    pub(super) fn abort(&self) {
        if let Some(handle) = &self.0 {
            handle.abort();
        }
    }
}

impl SessionTasks {
    pub(super) fn startup_guard(&self) -> StartupGuard<'_> {
        StartupGuard {
            tasks: self,
            committed: false,
        }
    }

    pub(super) fn open(&self) -> Result<(), String> {
        let mut state = self.0.lock().map_err(|_| "media task registry poisoned")?;
        if !state.tasks.is_empty() {
            return Err("finish media cleanup before starting another room".into());
        }
        state.accepting = true;
        Ok(())
    }

    pub(super) fn is_open(&self) -> bool {
        self.0
            .lock()
            .expect("media task registry poisoned")
            .accepting
    }

    pub(super) fn spawn(&self, future: impl Future<Output = ()> + Send + 'static) -> TaskHandle {
        let mut state = self.0.lock().expect("media task registry poisoned");
        if !state.accepting {
            return TaskHandle(None);
        }
        while let Some(result) = state.tasks.try_join_next() {
            Self::report(result);
        }
        let runtime = tokio::runtime::Handle::try_current()
            .unwrap_or_else(|_| tauri::async_runtime::handle().inner().clone());
        TaskHandle(Some(state.tasks.spawn_on(future, &runtime)))
    }

    pub(super) async fn run_blocking<T: Send + 'static>(
        &self,
        work: impl FnOnce() -> T + Send + 'static,
    ) -> Result<T, String> {
        let (send, receive) = tokio::sync::oneshot::channel();
        {
            let mut state = self.0.lock().map_err(|_| "media task registry poisoned")?;
            if !state.accepting {
                return Err("media session is closing".into());
            }
            let runtime = tokio::runtime::Handle::try_current()
                .unwrap_or_else(|_| tauri::async_runtime::handle().inner().clone());
            state.tasks.spawn_blocking_on(
                move || {
                    let result = work();
                    let _ = send.send(result);
                },
                &runtime,
            );
        }
        receive
            .await
            .map_err(|_| "media initialization task did not complete".into())
    }

    pub(super) async fn shutdown(&self) {
        self.close();
        // poll_join_next keeps unjoined tasks in the registry if this future
        // is cancelled. Never hold the synchronous mutex across an await.
        while let Some(result) = poll_fn(|cx| {
            self.0
                .lock()
                .expect("media task registry poisoned")
                .tasks
                .poll_join_next(cx)
        })
        .await
        {
            Self::report(result);
        }
    }

    fn close(&self) {
        let mut state = self.0.lock().expect("media task registry poisoned");
        state.accepting = false;
        state.tasks.abort_all();
    }

    fn report(result: Result<(), tokio::task::JoinError>) {
        if let Err(error) = result {
            if !error.is_cancelled() {
                log::warn!("media task failed: {error}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::sync::oneshot;

    #[tokio::test]
    async fn cleanup_waits_for_native_initialization_after_caller_cancellation() {
        let tasks = SessionTasks::default();
        tasks.open().expect("open");
        let (started, ready) = oneshot::channel();
        let (release, released) = oneshot::channel();
        let mut request = Box::pin(tasks.run_blocking(move || {
            started.send(()).expect("native init starts");
            released.blocking_recv().expect("native init release");
            42
        }));
        assert!(futures::poll!(&mut request).is_pending());
        ready.await.expect("native init running");
        drop(request);
        let mut cleanup = Box::pin(tasks.shutdown());
        assert!(futures::poll!(&mut cleanup).is_pending());
        assert!(tasks.open().is_err());
        release.send(()).expect("finish native init");
        cleanup.await;
        tasks
            .open()
            .expect("next session after native init finished");
    }

    #[tokio::test]
    async fn native_callback_thread_can_register_owned_work() {
        let tasks = Arc::new(SessionTasks::default());
        tasks.open().expect("open");
        let callback_tasks = tasks.clone();
        let (started, ready) = oneshot::channel();
        std::thread::spawn(move || {
            callback_tasks.spawn(async move {
                started.send(()).expect("native callback work");
            });
        })
        .join()
        .expect("native callback thread");
        ready.await.expect("task ran on app runtime");
        tasks.shutdown().await;
    }

    #[tokio::test]
    async fn abandoned_startup_closes_admission_and_keeps_tasks_joinable() {
        let tasks = SessionTasks::default();
        tasks.open().expect("open");
        let (owner, released) = oneshot::channel::<()>();
        {
            let _startup = tasks.startup_guard();
            tasks.spawn(async move {
                let _owner = owner;
                std::future::pending::<()>().await;
            });
        }
        assert!(!tasks.is_open());
        assert!(tasks.open().is_err());
        tasks.shutdown().await;
        assert!(released.await.is_err());
        tasks.open().expect("reopen");
        {
            let mut startup = tasks.startup_guard();
            startup.commit();
        }
        assert!(tasks.is_open());
    }

    #[tokio::test]
    async fn shutdown_joins_children_and_rejects_late_spawns() {
        let tasks = Arc::new(SessionTasks::default());
        tasks.open().expect("open");
        let (owner, mut released) = oneshot::channel::<()>();
        let (started, ready) = oneshot::channel();
        let children = tasks.clone();
        tasks.spawn(async move {
            children.spawn(async move {
                let _owner = owner;
                std::future::pending::<()>().await;
            });
            started.send(()).expect("signal child registered");
        });
        ready.await.expect("child registered");
        tasks.shutdown().await;
        assert!(matches!(
            released.try_recv(),
            Err(oneshot::error::TryRecvError::Closed)
        ));
        let (late, late_receiver) = oneshot::channel();
        tasks.spawn(async move {
            late.send(()).expect("must not run");
        });
        assert!(late_receiver.await.is_err());
        assert!(!tasks.is_open());
        tasks.open().expect("reopen only after drain");
    }

    #[tokio::test]
    async fn interrupted_cleanup_keeps_tasks_and_blocks_reopen() {
        let tasks = SessionTasks::default();
        tasks.open().expect("open");
        let (owner, released) = oneshot::channel::<()>();
        tasks.spawn(async move {
            let _owner = owner;
            std::future::pending::<()>().await;
        });
        {
            let mut cleanup = Box::pin(tasks.shutdown());
            assert!(futures::poll!(&mut cleanup).is_pending());
        }
        assert!(!tasks.is_open());
        assert!(tasks.open().is_err());
        tasks.shutdown().await;
        assert!(released.await.is_err());
        tasks.open().expect("cleanup retry drained tasks");
    }
}
