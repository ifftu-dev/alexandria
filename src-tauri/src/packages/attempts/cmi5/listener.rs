//! The session endpoint's socket: HTTP/1.1 on `127.0.0.1`, one listener per
//! open session. Everything it answers comes from [`super::endpoint`]; the
//! listener holds no session state, so it can be dropped and bound again on
//! the same port (iOS may reclaim a suspended app's listening socket).

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use http_body_util::{BodyExt, Full, Limited};
use hyper::body::{Bytes, Incoming};
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;
use tauri::http::{Request, Response, StatusCode};
use tokio::net::{TcpListener, TcpSocket, TcpStream};
use tokio::task::AbortHandle;

use super::endpoint::{self, MAX_BODY_BYTES};
use super::{end, Cmi5Sessions, Live};
use crate::package_serving::Launches;
use crate::packages::PackageDb;

/// A running listener: its accept loop and every connection it took.
pub(super) struct Listener {
    accept: AbortHandle,
    connections: Arc<Mutex<Vec<AbortHandle>>>,
}

impl Listener {
    /// Stops accepting and drops every open connection.
    pub(super) fn stop(self) {
        self.accept.abort();
        for connection in self
            .connections
            .lock()
            .expect("cmi5 listener lock poisoned")
            .drain(..)
        {
            connection.abort();
        }
    }
}

/// Binds `127.0.0.1:<port>` (0: the system picks). Address reuse is set on
/// Unix only, so a reclaimed listener can come back on its port; on Windows
/// it would let another program take the port.
pub(super) fn bind(port: u16) -> std::io::Result<TcpListener> {
    let socket = TcpSocket::new_v4()?;
    #[cfg(unix)]
    socket.set_reuseaddr(true)?;
    socket.bind(SocketAddr::from((Ipv4Addr::LOCALHOST, port)))?;
    socket.listen(64)
}

/// Serves the session on `listener` until stopped.
pub(super) fn serve<D>(listener: TcpListener, db: Arc<D>, live: Arc<Live>) -> Listener
where
    D: PackageDb + Send + Sync + 'static,
{
    let connections: Arc<Mutex<Vec<AbortHandle>>> = Arc::default();
    let taken = connections.clone();
    let accept = tokio::spawn(async move {
        loop {
            let stream = match listener.accept().await {
                Ok((stream, _)) => stream,
                Err(error) => {
                    // A reclaimed socket keeps failing; the resume check
                    // finds it dead and binds again.
                    log::warn!("cmi5 endpoint accept failed: {error}");
                    tokio::time::sleep(Duration::from_millis(200)).await;
                    continue;
                }
            };
            let db = db.clone();
            let live = live.clone();
            let connection = tokio::spawn(async move {
                let service = service_fn(move |request: Request<Incoming>| {
                    let db = db.clone();
                    let live = live.clone();
                    async move { Ok::<_, std::convert::Infallible>(answer(&*db, &live, request).await) }
                });
                let _ = hyper::server::conn::http1::Builder::new()
                    .keep_alive(true)
                    .serve_connection(TokioIo::new(stream), service)
                    .await;
            });
            let mut list = taken.lock().expect("cmi5 listener lock poisoned");
            list.retain(|handle| !handle.is_finished());
            list.push(connection.abort_handle());
        }
    });
    Listener {
        accept: accept.abort_handle(),
        connections,
    }
}

async fn answer<D: PackageDb>(
    db: &D,
    live: &Live,
    request: Request<Incoming>,
) -> Response<Full<Bytes>> {
    let (parts, body) = request.into_parts();
    let response = match Limited::new(body, MAX_BODY_BYTES).collect().await {
        Ok(collected) => {
            let request = Request::from_parts(parts, collected.to_bytes().to_vec());
            endpoint::handle(db, live, request).await
        }
        Err(_) => Response::builder()
            .status(StatusCode::PAYLOAD_TOO_LARGE)
            .header(
                "Access-Control-Allow-Origin",
                crate::package_serving::origin(),
            )
            .body(Vec::new())
            .expect("static response"),
    };
    response.map(|body| Full::new(Bytes::from(body)))
}

/// Brings every open session's endpoint back after the app resumes: a
/// listener the system reclaimed while the app was suspended is bound again
/// on its port, so the unit carries on with the same endpoint and token. A
/// session whose port cannot be bound again ends abnormally (`abandoned`);
/// its id is returned so the player can be told.
pub async fn revive<D>(db: Arc<D>, sessions: &Cmi5Sessions, launches: &Launches) -> Vec<String>
where
    D: PackageDb + Send + Sync + 'static,
{
    let mut ended = Vec::new();
    for live in sessions.all() {
        let alive = tokio::time::timeout(
            Duration::from_millis(500),
            TcpStream::connect((Ipv4Addr::LOCALHOST, live.port)),
        )
        .await
        .is_ok_and(|connected| connected.is_ok());
        if alive {
            continue;
        }
        if let Some(old) = live
            .listener
            .lock()
            .expect("cmi5 session lock poisoned")
            .take()
        {
            old.stop();
        }
        match bind(live.port) {
            Ok(listener) => {
                let serving = serve(listener, db.clone(), live.clone());
                *live.listener.lock().expect("cmi5 session lock poisoned") = Some(serving);
            }
            Err(error) => {
                log::warn!("cmi5 endpoint could not bind again: {error}");
                if let Err(error) = end(&*db, sessions, launches, &live.id).await {
                    log::warn!("cmi5 session not ended: {error}");
                }
                ended.push(live.id.clone());
            }
        }
    }
    ended
}
