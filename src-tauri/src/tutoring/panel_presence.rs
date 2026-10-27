//! Cloud liveness belongs to the connected panel runtime, alongside its media tasks.
use std::future::Future;
use std::sync::Arc;

use serde_json::Value;
use tokio::sync::oneshot;

use super::tasks::{SessionTasks, TaskHandle};
use crate::commands::interview_panel::{panel_request, PanelLink};
use crate::profile::scope::ProfileLease;

pub(crate) struct Publication {
    pub ticket: String,
    pub opening_id: Option<String>,
    pub previous_opening_id: Option<String>,
}

#[derive(Clone)]
struct Identity {
    link: PanelLink,
    opening_id: String,
    presence_id: String,
}

// Constructed before task admission: even cancellation before the first poll
// withdraws the exact operation, including ambiguous remote publication.
struct WithdrawOnDrop(Identity);
impl Drop for WithdrawOnDrop {
    fn drop(&mut self) {
        let identity = self.0.clone();
        tokio::spawn(async move {
            let suffix = format!(
                "/room/presence/{}/{}",
                identity.opening_id, identity.presence_id
            );
            let _ = panel_request(&identity.link, reqwest::Method::DELETE, &suffix, None).await;
        });
    }
}

pub(super) fn attach(
    tasks: &Arc<SessionTasks>,
    handle: &live::rooms::RoomHandle,
    presence: &mut Option<TaskHandle>,
    endpoint: iroh::Endpoint,
    lease: ProfileLease,
    link: PanelLink,
    publication: Publication,
) -> Result<oneshot::Receiver<Result<Value, String>>, String> {
    let ticket: live::rooms::RoomTicket = publication
        .ticket
        .parse()
        .map_err(|_| "invalid room ticket")?;
    if !tasks.is_open()
        || handle.is_closed()
        || endpoint.is_closed()
        || ticket.topic_id != handle.ticket().topic_id
    {
        return Err("the panel room is not connected".into());
    }
    if presence.as_ref().is_some_and(|task| !task.is_finished()) {
        return Err("this panel runtime already has a presence".into());
    }
    let handle = handle.clone();
    let disconnected = async move {
        tokio::select! { _ = handle.closed() => {}, _ = endpoint.closed() => {} }
    };
    let (task, receive) = start(tasks, lease, disconnected, link, publication);
    *presence = Some(task);
    Ok(receive)
}

pub(super) fn start(
    tasks: &Arc<SessionTasks>,
    lease: ProfileLease,
    disconnected: impl Future<Output = ()> + Send + 'static,
    link: PanelLink,
    publication: Publication,
) -> (TaskHandle, oneshot::Receiver<Result<Value, String>>) {
    let (send, receive) = oneshot::channel();
    let joining = publication.opening_id.is_some();
    let mut guard = WithdrawOnDrop(Identity {
        link,
        opening_id: publication
            .opening_id
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
        presence_id: uuid::Uuid::new_v4().to_string(),
    });
    let task = tasks.spawn(async move {
        let work = async {
            let identity = &guard.0;
            let mut body = serde_json::json!({ "openingId": identity.opening_id, "presenceId": identity.presence_id, "connected": true });
            let suffix = if joining { "/room/presence" } else {
                body["ticket"] = Value::String(publication.ticket);
                body["previousOpeningId"] = publication.previous_opening_id.map(Value::String).unwrap_or(Value::Null);
                "/room"
            };
            let result = panel_request(&identity.link, reqwest::Method::POST, suffix, Some(&body)).await;
            let result = match result {
                Ok(_) if joining => panel_request(&identity.link, reqwest::Method::GET, "", None).await,
                result => result,
            };
            let admitted = result.is_ok();
            let _ = send.send(result);
            if !admitted { return; }
            let period = std::time::Duration::from_secs(30);
            let mut heartbeat = tokio::time::interval_at(tokio::time::Instant::now() + period, period);
            heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                heartbeat.tick().await;
                let body = serde_json::json!({ "openingId": guard.0.opening_id, "presenceId": guard.0.presence_id, "connected": true });
                if let Err(error) = panel_request(&guard.0.link, reqwest::Method::PUT, "/room/presence", Some(&body)).await {
                    if error.starts_with("directory returned 409") {
                        // The old lease can never be revived. Only another
                        // panelist keeping this exact opening live permits a
                        // fresh admission for our still-connected runtime.
                        let Ok(panel) = panel_request(&guard.0.link, reqwest::Method::GET, "", None).await else { return; };
                        if panel["interviewId"] != guard.0.link.interview_id
                            || panel["me"]["id"] != guard.0.link.panelist_id
                            || panel["room"]["openingId"] != guard.0.opening_id
                            || panel["room"]["live"] != true { return; }
                        let next = WithdrawOnDrop(Identity { link: guard.0.link.clone(), opening_id: guard.0.opening_id.clone(), presence_id: uuid::Uuid::new_v4().to_string() });
                        guard = next; // withdraw the original connection, never other panelists
                        let body = serde_json::json!({ "openingId": guard.0.opening_id, "presenceId": guard.0.presence_id, "connected": true });
                        if panel_request(&guard.0.link, reqwest::Method::POST, "/room/presence", Some(&body)).await.is_err() { return; }
                    } else if error.starts_with("directory returned 4") {
                        return;
                    }
                    // Network/5xx failures do not assert a successful renewal.
                    // Cloud expires the lease after 90s; a later 409 reconciles.
                }
            }
        };
        tokio::select! {
            biased;
            _ = disconnected => {},
            _ = lease.run_until_closed(work) => {},
        }
        drop(guard);
    });
    (task, receive)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::operations::ProfileOperations;
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::sync::mpsc;

    type Server = (
        PanelLink,
        mpsc::UnboundedReceiver<(String, Value)>,
        tokio::task::JoinHandle<()>,
    );
    async fn server() -> Server {
        server_case(false, false, Value::Null).await
    }
    async fn server_case(hold_publication: bool, conflict_heartbeat: bool, panel: Value) -> Server {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let link = PanelLink {
            cloud: format!("http://{}", listener.local_addr().unwrap()),
            interview_id: "11111111-1111-4111-8111-111111111111".into(),
            panelist_id: "22222222-2222-4222-8222-222222222222".into(),
            token: "original-authority".into(),
        };
        let (send, receive) = mpsc::unbounded_channel();
        let task = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let send = send.clone();
                let panel = panel.clone();
                tokio::spawn(async move {
                    let mut bytes = Vec::new();
                    loop {
                        let mut buffer = [0; 4096];
                        let n = socket.read(&mut buffer).await.unwrap();
                        if n == 0 {
                            return;
                        }
                        bytes.extend_from_slice(&buffer[..n]);
                        if let Some(end) = bytes.windows(4).position(|s| s == b"\r\n\r\n") {
                            let headers = String::from_utf8_lossy(&bytes[..end]);
                            let len = headers
                                .lines()
                                .find_map(|l| l.strip_prefix("content-length: "))
                                .and_then(|l| l.parse::<usize>().ok())
                                .unwrap_or(0);
                            if bytes.len() < end + 4 + len {
                                continue;
                            }
                            assert!(headers.contains("authorization: Panelist original-authority"));
                            let path = headers.lines().next().unwrap().to_string();
                            let body =
                                serde_json::from_slice(&bytes[end + 4..]).unwrap_or(Value::Null);
                            let _ = send.send((path.clone(), body));
                            if hold_publication && path.starts_with("POST ") {
                                std::future::pending::<()>().await;
                            }
                            let response = if path.starts_with("DELETE ") {
                                "HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n"
                            } else {
                                "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}"
                            };
                            let response = if path.starts_with("PUT ") && conflict_heartbeat {
                                "HTTP/1.1 409 Conflict\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}".to_string()
                            } else if path.starts_with("GET ") && !panel.is_null() {
                                let body = panel.to_string();
                                format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())
                            } else {
                                response.to_string()
                            };
                            let _ = socket.write_all(response.as_bytes()).await;
                            break;
                        }
                    }
                });
            }
        });
        (link, receive, task)
    }

    #[tokio::test]
    async fn abandoning_ambiguous_publication_sends_original_tombstone() {
        let (link, mut requests, server) = server_case(true, false, Value::Null).await;
        let operations = ProfileOperations::default();
        operations
            .activate(async { Ok(()) }, async { Ok(()) })
            .await
            .unwrap();
        let tasks = Arc::new(SessionTasks::default());
        tasks.open().unwrap();
        let (_, ready) = start(
            &tasks,
            operations.admit(&operations.session().unwrap()).unwrap(),
            std::future::pending(),
            link,
            Publication {
                ticket: "ticket".into(),
                opening_id: None,
                previous_opening_id: None,
            },
        );
        let (_, publication) = requests.recv().await.unwrap();
        tasks.shutdown().await;
        assert!(ready.await.is_err());
        let (leave, _) = tokio::time::timeout(Duration::from_secs(2), requests.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(leave.ends_with(&format!(
            "/room/presence/{}/{} HTTP/1.1",
            publication["openingId"].as_str().unwrap(),
            publication["presenceId"].as_str().unwrap()
        )));
        operations.lock(async { Ok(()) }).await.unwrap();
        server.abort();
    }

    #[tokio::test]
    async fn expired_own_lease_rejoins_only_the_same_opening_kept_live_by_another_panelist() {
        for current_opening in [
            "44444444-4444-4444-8444-444444444444",
            "55555555-5555-4555-8555-555555555555",
        ] {
            let panel = serde_json::json!({"interviewId":"11111111-1111-4111-8111-111111111111", "me":{"id":"22222222-2222-4222-8222-222222222222"}, "room":{"openingId":current_opening,"live":true}});
            let (link, mut requests, server) = server_case(false, true, panel).await;
            let operations = ProfileOperations::default();
            operations
                .activate(async { Ok(()) }, async { Ok(()) })
                .await
                .unwrap();
            let tasks = Arc::new(SessionTasks::default());
            tasks.open().unwrap();
            let (_, ready) = start(
                &tasks,
                operations.admit(&operations.session().unwrap()).unwrap(),
                std::future::pending(),
                link,
                Publication {
                    ticket: "ticket".into(),
                    opening_id: Some("44444444-4444-4444-8444-444444444444".into()),
                    previous_opening_id: None,
                },
            );
            ready.await.unwrap().unwrap();
            let (_, original) = requests.recv().await.unwrap();
            requests.recv().await.unwrap(); // initial panel view
            tokio::time::pause();
            tokio::time::advance(Duration::from_secs(31)).await;
            tokio::time::resume();
            let (heartbeat, _) = tokio::time::timeout(Duration::from_secs(2), requests.recv())
                .await
                .unwrap()
                .unwrap();
            assert!(heartbeat.starts_with("PUT "));
            assert!(requests.recv().await.unwrap().0.starts_with("GET "));
            let mut replacement = None;
            let expected = if current_opening.starts_with('4') {
                2
            } else {
                1
            };
            for _ in 0..expected {
                let (path, body) = tokio::time::timeout(Duration::from_secs(2), requests.recv())
                    .await
                    .unwrap()
                    .unwrap();
                if path.starts_with("DELETE ") {
                    assert!(path.ends_with(&format!(
                        "/room/presence/44444444-4444-4444-8444-444444444444/{} HTTP/1.1",
                        original["presenceId"].as_str().unwrap()
                    )));
                } else {
                    assert!(path.starts_with("POST ") && path.ends_with("/room/presence HTTP/1.1"));
                    assert_eq!(body["openingId"], original["openingId"]);
                    assert_ne!(body["presenceId"], original["presenceId"]);
                    replacement = Some(body);
                }
            }
            assert_eq!(replacement.is_some(), current_opening.starts_with('4'));
            tasks.shutdown().await;
            server.abort();
        }
    }

    #[tokio::test]
    async fn a_connected_join_renews_every_thirty_seconds_until_transport_stops() {
        let (link, mut requests, server) = server().await;
        let operations = ProfileOperations::default();
        operations
            .activate(async { Ok(()) }, async { Ok(()) })
            .await
            .unwrap();
        let lease = operations.admit(&operations.session().unwrap()).unwrap();
        let tasks = Arc::new(SessionTasks::default());
        tasks.open().unwrap();
        let (disconnect, disconnected) = oneshot::channel();
        let (_task, ready) = start(
            &tasks,
            lease,
            async {
                let _ = disconnected.await;
            },
            link,
            Publication {
                ticket: "connected-ticket".into(),
                opening_id: Some("44444444-4444-4444-8444-444444444444".into()),
                previous_opening_id: None,
            },
        );
        ready.await.unwrap().unwrap();
        let (join, body) = requests.recv().await.unwrap();
        assert!(join.ends_with("/room/presence HTTP/1.1"));
        assert!(body.get("ticket").is_none());
        assert_eq!(body["openingId"], "44444444-4444-4444-8444-444444444444");
        assert!(requests.recv().await.unwrap().0.starts_with("GET "));
        tokio::time::pause();
        tokio::time::advance(Duration::from_secs(29)).await;
        assert!(requests.try_recv().is_err(), "admission is not a heartbeat");
        tokio::time::advance(Duration::from_secs(2)).await;
        tokio::time::resume();
        let (heartbeat, renewed) = tokio::time::timeout(Duration::from_secs(2), requests.recv())
            .await
            .expect("connected runtime renews without a mounted page")
            .unwrap();
        assert!(heartbeat.starts_with("PUT "));
        assert_eq!(renewed, body);
        disconnect.send(()).unwrap();
        let (leave, _) = tokio::time::timeout(Duration::from_secs(2), requests.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(leave.ends_with(&format!(
            "/room/presence/44444444-4444-4444-8444-444444444444/{} HTTP/1.1",
            body["presenceId"].as_str().unwrap()
        )));
        tasks.shutdown().await;
        server.abort();
    }

    #[tokio::test]
    async fn profile_close_withdraws_original_presence_and_does_not_wait_for_room_shutdown() {
        let (link, mut requests, server) = server().await;
        let operations = ProfileOperations::default();
        operations
            .activate(async { Ok(()) }, async { Ok(()) })
            .await
            .unwrap();
        let lease = operations.admit(&operations.session().unwrap()).unwrap();
        let tasks = Arc::new(SessionTasks::default());
        tasks.open().unwrap();
        let (_task, ready) = start(
            &tasks,
            lease,
            std::future::pending(),
            link,
            Publication {
                ticket: "connected-ticket".into(),
                opening_id: None,
                previous_opening_id: Some("33333333-3333-4333-8333-333333333333".into()),
            },
        );
        let (path, body) = tokio::time::timeout(Duration::from_secs(2), requests.recv())
            .await
            .expect("publication starts")
            .unwrap();
        assert!(path.ends_with("/room HTTP/1.1"));
        assert_eq!(body["ticket"], "connected-ticket");
        assert_eq!(body["connected"], true);
        assert_eq!(
            body["previousOpeningId"],
            "33333333-3333-4333-8333-333333333333"
        );
        let opening = body["openingId"].as_str().unwrap();
        let presence = body["presenceId"].as_str().unwrap();
        assert!(uuid::Uuid::parse_str(opening).is_ok());
        assert!(uuid::Uuid::parse_str(presence).is_ok());
        ready.await.unwrap().unwrap();
        tokio::time::timeout(Duration::from_secs(2), operations.lock(async { Ok(()) }))
            .await
            .expect("profile admission drains without stop_jobs")
            .unwrap();
        let (leave, _) = tokio::time::timeout(Duration::from_secs(2), requests.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(leave.starts_with("DELETE "));
        assert!(leave.ends_with(&format!("/room/presence/{opening}/{presence} HTTP/1.1")));
        tasks.shutdown().await;
        server.abort();
    }
}
