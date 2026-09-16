use std::future::Future;
use std::ops::Deref;
use std::sync::{Arc, Mutex};

use tauri::ipc::{CommandArg, CommandItem, InvokeError};
use tokio::sync::Notify;

use crate::AppState;

const SESSION_HEADER: &str = "x-alexandria-profile-session";

#[derive(Default)]
struct AdmissionState {
    session: String,
    accepting: bool,
    readers: usize,
}

#[derive(Default)]
struct AdmissionInner {
    state: Mutex<AdmissionState>,
    idle: Notify,
    closed: Notify,
}

#[derive(Clone, Default)]
pub(crate) struct Admission(Arc<AdmissionInner>);

impl Admission {
    pub(crate) fn revision(&self) -> String {
        self.0
            .state
            .lock()
            .expect("profile admission poisoned")
            .session
            .clone()
    }

    pub(crate) fn begin_start(&self, expected_revision: &str) -> Option<String> {
        let mut state = self.0.state.lock().expect("profile admission poisoned");
        if state.session != expected_revision {
            return None;
        }
        state.accepting = false;
        state.session = uuid::Uuid::new_v4().to_string();
        let session = state.session.clone();
        drop(state);
        self.0.closed.notify_waiters();
        Some(session)
    }

    pub(crate) fn close(&self) -> String {
        let mut state = self.0.state.lock().expect("profile admission poisoned");
        state.accepting = false;
        state.session = uuid::Uuid::new_v4().to_string();
        let session = state.session.clone();
        drop(state);
        self.0.closed.notify_waiters();
        session
    }

    pub(crate) fn open(&self, ticket: &str) -> bool {
        let mut state = self.0.state.lock().expect("profile admission poisoned");
        if state.session != ticket || state.readers != 0 {
            return false;
        }
        state.accepting = true;
        true
    }

    pub(crate) fn session(&self) -> Option<String> {
        let state = self.0.state.lock().expect("profile admission poisoned");
        state.accepting.then(|| state.session.clone())
    }

    pub(crate) fn admit(&self, expected: &str) -> Result<ProfileLease, String> {
        let mut state = self
            .0
            .state
            .lock()
            .map_err(|_| "profile admission poisoned")?;
        if !state.accepting || state.session != expected {
            return Err("profile session is locked or has changed".to_string());
        }
        state.readers = state
            .readers
            .checked_add(1)
            .ok_or("too many profile operations")?;
        Ok(ProfileLease {
            inner: Arc::new(LeaseInner {
                admission: self.0.clone(),
                session: expected.to_owned(),
            }),
        })
    }

    pub(crate) async fn drain(&self) {
        loop {
            let notified = self.0.idle.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self
                .0
                .state
                .lock()
                .expect("profile admission poisoned")
                .readers
                == 0
            {
                return;
            }
            notified.await;
        }
    }
}

struct LeaseInner {
    admission: Arc<AdmissionInner>,
    session: String,
}

impl Drop for LeaseInner {
    fn drop(&mut self) {
        let mut state = self
            .admission
            .state
            .lock()
            .expect("profile admission poisoned");
        state.readers -= 1;
        if state.readers == 0 {
            self.admission.idle.notify_waiters();
        }
    }
}

#[derive(Clone)]
pub struct ProfileLease {
    inner: Arc<LeaseInner>,
}

impl ProfileLease {
    pub(crate) fn is_current(&self) -> bool {
        let state = self
            .inner
            .admission
            .state
            .lock()
            .expect("profile admission poisoned");
        state.accepting && state.session == self.inner.session
    }

    async fn closed(&self) {
        loop {
            let notice = self.inner.admission.closed.notified();
            tokio::pin!(notice);
            notice.as_mut().enable();
            {
                let state = self
                    .inner
                    .admission
                    .state
                    .lock()
                    .expect("profile admission poisoned");
                if !state.accepting || state.session != self.inner.session {
                    return;
                }
            }
            notice.await;
        }
    }

    /// Cancel only work whose future owns its resources and is safe to drop.
    /// In particular, never use this to detach spawn_blocking or unjoined jobs.
    /// The work future is dropped before this lease is released, so cleanup
    /// cannot replace profile resources underneath its cancellation handlers.
    pub(crate) async fn run_until_closed<T>(self, work: impl Future<Output = T>) -> Option<T> {
        tokio::select! {
            biased;
            _ = self.closed() => None,
            result = work => Some(result),
        }
    }
}

impl<'de, R: tauri::Runtime> CommandArg<'de, R> for ProfileLease {
    fn from_command(command: CommandItem<'de, R>) -> Result<Self, InvokeError> {
        let state = command
            .message
            .state_ref()
            .try_get::<AppState>()
            .ok_or_else(|| InvokeError::from("application state unavailable"))?;
        let session = command
            .message
            .headers()
            .get(SESSION_HEADER)
            .and_then(|value| value.to_str().ok())
            .ok_or_else(|| InvokeError::from("profile session header is required"))?;
        state
            .profile_operations
            .admit(session)
            .map_err(InvokeError::from)
    }
}

pub struct ProfileState<'a, T: Send + Sync + 'static> {
    state: tauri::State<'a, T>,
    lease: ProfileLease,
}

impl<T: Send + Sync + 'static> Clone for ProfileState<'_, T> {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
            lease: self.lease.clone(),
        }
    }
}

impl<'a, T: Send + Sync + 'static> Deref for ProfileState<'a, T> {
    type Target = tauri::State<'a, T>;

    fn deref(&self) -> &Self::Target {
        &self.state
    }
}

impl<'de, R: tauri::Runtime> CommandArg<'de, R> for ProfileState<'de, AppState> {
    fn from_command(command: CommandItem<'de, R>) -> Result<Self, InvokeError> {
        let state = command
            .message
            .state_ref()
            .try_get::<AppState>()
            .ok_or_else(|| InvokeError::from("application state unavailable"))?;
        let lease = ProfileLease::from_command(command)?;
        Ok(Self { state, lease })
    }
}

impl ProfileState<'_, AppState> {
    pub(crate) fn profile_lease(&self) -> ProfileLease {
        self.lease.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use tauri::test::{get_ipc_response, mock_builder, mock_context, noop_assets, MockRuntime};

    #[derive(Default)]
    struct Probe {
        calls: AtomicUsize,
        started: Notify,
        release: Notify,
    }

    #[tauri::command]
    async fn scoped_probe(
        state: ProfileState<'_, AppState>,
        probe: tauri::State<'_, Arc<Probe>>,
    ) -> Result<String, String> {
        probe.calls.fetch_add(1, Ordering::SeqCst);
        probe.started.notify_one();
        probe.release.notified().await;
        Ok(state.app_data_dir.to_string_lossy().into_owned())
    }

    async fn request(
        webview: &tauri::WebviewWindow<MockRuntime>,
        session: Option<&str>,
    ) -> Result<String, serde_json::Value> {
        let mut headers = tauri::http::HeaderMap::new();
        if let Some(session) = session {
            headers.insert(SESSION_HEADER, session.parse().expect("header value"));
        }
        let webview = webview.clone();
        tokio::task::spawn_blocking(move || {
            get_ipc_response(
                &webview,
                tauri::webview::InvokeRequest {
                    cmd: "scoped_probe".into(),
                    callback: tauri::ipc::CallbackFn(0),
                    error: tauri::ipc::CallbackFn(1),
                    url: if cfg!(any(target_os = "windows", target_os = "android")) {
                        "http://tauri.localhost"
                    } else {
                        "tauri://localhost"
                    }
                    .parse()
                    .expect("local URL"),
                    body: tauri::ipc::InvokeBody::default(),
                    headers,
                    invoke_key: tauri::test::INVOKE_KEY.to_string(),
                },
            )
            .map(|body| body.deserialize::<String>().expect("string response"))
        })
        .await
        .expect("IPC task")
    }

    #[tokio::test]
    async fn tauri_dispatch_rejects_stale_sessions_and_holds_lease_across_await() {
        let directory = tempfile::TempDir::new().expect("temporary app directory");
        let state = super::super::lifecycle_tests::state_in(directory.path());
        let operations = state.profile_operations.clone();
        let state = Arc::try_unwrap(state).unwrap_or_else(|_| panic!("unique app state"));
        let probe = Arc::new(Probe::default());
        let app = mock_builder()
            .manage(state)
            .manage(probe.clone())
            .invoke_handler(tauri::generate_handler![scoped_probe])
            .build(mock_context(noop_assets()))
            .expect("mock app");
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .expect("mock webview");
        operations
            .activate(async { Ok(()) }, async { Ok(()) })
            .await
            .expect("activate first profile");
        let first = operations.session().expect("first session");
        assert_eq!(
            request(&webview, None).await,
            Err(serde_json::json!("profile session header is required"))
        );
        assert!(request(&webview, Some("wrong-session")).await.is_err());
        assert_eq!(probe.calls.load(Ordering::SeqCst), 0);

        let mut admitted = Box::pin(request(&webview, Some(&first)));
        assert!(futures::poll!(&mut admitted).is_pending());
        tokio::time::timeout(std::time::Duration::from_secs(5), probe.started.notified())
            .await
            .expect("command admitted");
        let (cleanup_started, mut cleanup) = tokio::sync::oneshot::channel();
        let mut lock = Box::pin(operations.lock(async move {
            cleanup_started.send(()).expect("cleanup signal");
            Ok(())
        }));
        assert!(futures::poll!(&mut lock).is_pending());
        assert!(operations.session().is_none());
        assert!(request(&webview, Some(&first)).await.is_err());
        assert!(matches!(
            cleanup.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ));
        assert_eq!(probe.calls.load(Ordering::SeqCst), 1);
        probe.release.notify_one();
        admitted.await.expect("admitted command finishes");
        lock.await.expect("drained lock completes");
        cleanup.await.expect("cleanup ran after command");

        operations
            .activate(async { Ok(()) }, async { Ok(()) })
            .await
            .expect("activate next profile");
        let second = operations.session().expect("second session");
        assert_ne!(first, second);
        assert!(request(&webview, Some(&first)).await.is_err());
        probe.release.notify_one();
        request(&webview, Some(&second))
            .await
            .expect("new session admitted");
        assert_eq!(probe.calls.load(Ordering::SeqCst), 2);
        operations.lock(async { Ok(()) }).await.expect("final lock");
    }

    #[tokio::test]
    async fn lock_refuses_new_work_and_waits_for_every_lease_clone() {
        let admission = Admission::default();
        let session = admission.close();
        assert!(admission.open(&session));
        let lease = admission.admit(&session).expect("admitted");
        let clone = lease.clone();
        admission.close();
        assert!(admission.admit(&session).is_err());
        drop(lease);
        let mut drain = Box::pin(admission.drain());
        assert!(futures::poll!(&mut drain).is_pending());
        drop(clone);
        drain.await;
    }

    #[test]
    fn old_session_cannot_access_reopened_profile() {
        let admission = Admission::default();
        let old = admission.close();
        assert!(admission.open(&old));
        let new = admission.close();
        assert!(!admission.open(&old));
        assert!(admission.open(&new));
        assert!(admission.admit(&old).is_err());
        assert!(admission.admit(&new).is_ok());
    }

    #[tokio::test]
    async fn closed_lease_does_not_start_ready_work_or_revive_after_reopen() {
        let admission = Admission::default();
        let first = admission.close();
        assert!(admission.open(&first));
        let lease = admission.admit(&first).unwrap();
        let next = admission.close();
        let calls = AtomicUsize::new(0);
        let result = lease
            .run_until_closed(async { calls.fetch_add(1, Ordering::SeqCst) })
            .await;
        assert!(result.is_none());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        admission.drain().await;
        assert!(admission.open(&next));
        assert_eq!(
            admission
                .admit(&next)
                .unwrap()
                .run_until_closed(async { 42 })
                .await,
            Some(42)
        );
    }

    #[tokio::test]
    async fn closing_wakes_every_waiter_and_drops_work_before_draining() {
        struct MarkDropped(Arc<AtomicUsize>);
        impl Drop for MarkDropped {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        let admission = Admission::default();
        let session = admission.close();
        assert!(admission.open(&session));
        let lease = admission.admit(&session).unwrap();
        let drops = Arc::new(AtomicUsize::new(0));
        let mut jobs = tokio::task::JoinSet::new();
        let (started_tx, mut started_rx) = tokio::sync::mpsc::channel(2);
        for _ in 0..2 {
            let owned = lease.clone();
            let marker = MarkDropped(drops.clone());
            let started = started_tx.clone();
            jobs.spawn(async move {
                owned
                    .run_until_closed(async move {
                        let _marker = marker;
                        started.send(()).await.unwrap();
                        std::future::pending::<()>().await;
                    })
                    .await
            });
        }
        drop(lease);
        for _ in 0..2 {
            tokio::time::timeout(std::time::Duration::from_secs(5), started_rx.recv())
                .await
                .unwrap()
                .unwrap();
        }
        admission.close();
        tokio::time::timeout(std::time::Duration::from_secs(5), admission.drain())
            .await
            .unwrap();
        assert_eq!(drops.load(Ordering::SeqCst), 2);
        while let Some(result) = jobs.join_next().await {
            assert!(result.unwrap().is_none());
        }
    }

    #[tokio::test]
    async fn cancellation_between_awaits_prevents_the_next_request() {
        let admission = Admission::default();
        let session = admission.close();
        assert!(admission.open(&session));
        let lease = admission.admit(&session).unwrap();
        let (started, receiving) = tokio::sync::oneshot::channel();
        let (release, released) = tokio::sync::oneshot::channel();
        let calls = Arc::new(AtomicUsize::new(0));
        let next_calls = calls.clone();
        let job = tokio::spawn(lease.run_until_closed(async move {
            started.send(()).unwrap();
            released.await.unwrap();
            next_calls.fetch_add(1, Ordering::SeqCst);
        }));
        receiving.await.unwrap();
        admission.close();
        let _ = release.send(());
        assert!(job.await.unwrap().is_none());
        admission.drain().await;
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }
}
