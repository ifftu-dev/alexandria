//! The call manager through its own interface, over the synthetic adapter.
//!
//! Nothing here opens a device. Lifecycle tests drive task admission and
//! shutdown without a room; room tests bind iroh endpoints on this machine
//! and never leave it (no relay, no address lookup beyond memory).

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::oneshot;

use super::events::RecordedEvents;
use super::manager::CallManager;
use super::media::synthetic::SyntheticMedia;

type Manager = CallManager<SyntheticMedia>;

fn manager() -> (Manager, Arc<RecordedEvents>) {
    let events = Arc::new(RecordedEvents::default());
    (
        CallManager::new(SyntheticMedia::new(), events.clone()),
        events,
    )
}

// ── Lifecycle without a room ───────────────────────────────────────

#[tokio::test]
async fn shutdown_joins_partial_startup_tasks_without_a_published_room() {
    let (manager, _) = manager();
    manager.tasks().open().expect("startup admission");
    let (owner, mut released) = oneshot::channel::<()>();
    manager.tasks().spawn(async move {
        let _owner = owner;
        std::future::pending::<()>().await;
    });
    assert!(!manager.has_session().await);
    manager.shutdown().await.expect("partial startup cleanup");
    assert!(matches!(
        released.try_recv(),
        Err(oneshot::error::TryRecvError::Closed)
    ));
    assert!(!manager.tasks().is_open());
    assert!(!manager.is_active().await);
    manager.shutdown().await.expect("idempotent cleanup");
}

#[tokio::test]
async fn interrupted_shutdown_still_blocks_task_admission_until_retry() {
    let (manager, _) = manager();
    manager.tasks().open().expect("startup admission");
    let (owner, released) = oneshot::channel::<()>();
    manager.tasks().spawn(async move {
        let _owner = owner;
        std::future::pending::<()>().await;
    });
    {
        let mut cleanup = Box::pin(manager.shutdown());
        assert!(futures::poll!(&mut cleanup).is_pending());
    }
    assert!(!manager.tasks().is_open());
    assert!(manager.tasks().open().is_err());
    manager.shutdown().await.expect("retry cleanup");
    assert!(released.await.is_err());
    manager.tasks().open().expect("next room may start");
    manager.shutdown().await.expect("finish fixture");
}

#[tokio::test]
async fn leaving_returns_while_a_blocked_native_task_finishes_in_the_background() {
    let (manager, _) = manager();
    manager.tasks().open().expect("startup admission");
    let (release, released) = std::sync::mpsc::channel::<()>();
    let (started, running) = oneshot::channel::<()>();
    let tasks = manager.tasks().clone();
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
    assert!(!manager.tasks().is_open());
    // No second room starts while the first one's camera may still be held.
    assert!(manager.tasks().open().is_err());

    release.send(()).expect("finish native work");
    native.await.expect("native task");
    for _ in 0..100 {
        if manager.tasks().open().is_ok() {
            manager.shutdown().await.expect("finish fixture");
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("cleanup never finished after the native work returned");
}

// ── Calls between two devices on this machine ──────────────────────

use iroh::address_lookup::MemoryLookup;
use iroh::protocol::Router;
use iroh::{Endpoint, EndpointAddr, RelayMode, TransportAddr};
use iroh_gossip::net::Gossip;
use live::Live;

use super::call_kind::CallKind;
use super::manager::{Network, RoomRequest};
use super::media::{DeviceSelection, MediaEvent};
use super::records::MemoryRecords;

/// An iroh node bound to loopback, findable only through a shared
/// in-memory address book: no relay, no DNS, nothing leaves the machine.
struct Node {
    endpoint: Endpoint,
    gossip: Gossip,
    live: Live,
    _router: Router,
}

impl Node {
    async fn bind(book: &MemoryLookup) -> Self {
        let endpoint = Endpoint::builder(iroh::endpoint::presets::Minimal)
            .relay_mode(RelayMode::Disabled)
            .address_lookup(book.clone())
            .bind_addr("127.0.0.1:0")
            .expect("loopback address")
            .bind()
            .await
            .expect("bind loopback endpoint");
        let socket = endpoint
            .bound_sockets()
            .into_iter()
            .find(|s| s.is_ipv4())
            .expect("an IPv4 socket");
        book.add_endpoint_info(EndpointAddr::from_parts(
            endpoint.id(),
            [TransportAddr::Ip(socket)],
        ));
        let gossip = Gossip::builder().spawn(endpoint.clone());
        let live = Live::new(endpoint.clone());
        let router = Router::builder(endpoint.clone())
            .accept(iroh_gossip::ALPN, gossip.clone())
            .accept(live::ALPN, live.protocol_handler())
            .spawn();
        Self {
            endpoint,
            gossip,
            live,
            _router: router,
        }
    }

    fn network(&self) -> Network {
        Network {
            endpoint: self.endpoint.clone(),
            gossip: self.gossip.clone(),
            live: self.live.clone(),
        }
    }
}

fn request(name: &str, kind: CallKind, context: Option<&str>) -> RoomRequest {
    RoomRequest {
        session_id: uuid::Uuid::new_v4().to_string(),
        title: format!("{name}'s call"),
        display_name: name.to_string(),
        kind,
        context_id: context.map(str::to_string),
        devices: DeviceSelection::default(),
        classroom_call: None,
    }
}

fn session_row(
    records: &MemoryRecords,
    id: &str,
) -> (String, Option<String>, String, Option<String>) {
    records.with(|db| {
        db.conn()
            .query_row(
                "SELECT kind, context_id, status, room_id FROM tutoring_sessions WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .expect("session row")
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn a_call_records_its_kind_and_refuses_reentry_as_another_kind() {
    let book = MemoryLookup::new();
    let node = Node::bind(&book).await;
    let records = MemoryRecords::migrated();
    let (manager, _) = manager();

    let panel = request("Panelist", CallKind::InterviewPanel, Some("int-7"));
    let id = panel.session_id.clone();
    let ticket = manager
        .create_room(panel, node.network(), &records)
        .await
        .expect("create");
    assert!(manager.is_active().await);
    assert!(manager.media().session_running());

    let (kind, context, status, room) = session_row(&records, &id);
    assert_eq!(kind, "interview_panel");
    assert_eq!(context.as_deref(), Some("int-7"));
    assert_eq!(status, "active");
    assert_eq!(
        room.as_deref(),
        Some(super::call_kind::room_of(&ticket).unwrap().as_str())
    );

    manager.leave_room(&records).await.expect("leave");
    assert!(!manager.is_active().await);
    assert!(!manager.media().session_running());
    let occupied: Option<i64> = records.with(|db| {
        db.conn()
            .query_row(
                "SELECT last_occupied_at FROM tutoring_sessions WHERE id = ?1",
                [&id],
                |r| r.get(0),
            )
            .unwrap()
    });
    assert!(
        occupied.is_some(),
        "leaving notes when the room was occupied"
    );

    // The same ticket as plain tutoring is refused before anything starts.
    let refused = manager
        .join_room(
            request("Panelist", CallKind::Tutoring, None),
            &ticket,
            node.network(),
            &records,
        )
        .await
        .expect_err("another kind is refused");
    assert!(refused.starts_with("CALL_KIND_MISMATCH"), "{refused}");
    assert!(refused.contains("interview"), "{refused}");
    assert!(!manager.is_active().await);
    assert!(!manager.media().session_running());

    // Under its own kind the room opens again.
    manager
        .join_room(
            request("Panelist", CallKind::InterviewPanel, Some("int-7")),
            &ticket,
            node.network(),
            &records,
        )
        .await
        .expect("same kind rejoins");
    assert!(manager.is_active().await);
    manager.shutdown().await.expect("cleanup");
}

#[tokio::test(flavor = "multi_thread")]
async fn chat_transcript_and_names_reach_the_other_device_and_never_echo() {
    let book = MemoryLookup::new();
    let (host_node, guest_node) = (Node::bind(&book).await, Node::bind(&book).await);
    let (host, host_events) = manager();
    let (guest, guest_events) = manager();
    let (host_records, guest_records) = (MemoryRecords::migrated(), MemoryRecords::migrated());

    let ticket = host
        .create_room(
            request("Host", CallKind::Tutoring, None),
            host_node.network(),
            &host_records,
        )
        .await
        .expect("host creates");
    guest
        .join_room(
            request("Guest", CallKind::Tutoring, None),
            &ticket,
            guest_node.network(),
            &guest_records,
        )
        .await
        .expect("guest joins");

    // The chat topic forms its neighbours once the room has met the peer,
    // so keep saying hello until it lands.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    let chat = loop {
        let _ = guest.send_chat("hello from the guest".into()).await;
        if let Some(chat) = host_events
            .wait_for("tutoring:chat", Duration::from_millis(300), |p| {
                p["text"] == "hello from the guest"
            })
            .await
        {
            break chat;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "chat never reached the host; host peers {:?}, guest peers {:?}, guest diag {:?}",
            host.peers().await,
            guest.peers().await,
            guest.diagnostics().await.map(|d| d
                .recent_logs
                .into_iter()
                .take(12)
                .collect::<Vec<_>>()),
        );
    };
    assert_eq!(chat["sender_name"], "Guest");
    assert_eq!(
        chat["sender"],
        guest_node.endpoint.id().to_string(),
        "attributed to the sending device"
    );

    guest
        .send_transcript("a caption".into(), Some(1.7))
        .await
        .expect("transcript");
    let caption = host_events
        .wait_for("tutoring:transcript", Duration::from_secs(10), |p| {
            p["text"] == "a caption"
        })
        .await
        .expect("transcript reaches the host");
    assert_eq!(caption["confidence"], 1.0, "confidence is clamped");
    assert!(
        host_events
            .payloads("tutoring:chat")
            .iter()
            .all(|p| p["text"] != "a caption"),
        "a transcript is never shown as chat"
    );

    let name = host_events
        // Names are re-announced every 15s; the first may go out before the
        // topic has a neighbour.
        .wait_for("tutoring:peer-name", Duration::from_secs(40), |p| {
            p["display_name"] == "Guest"
        })
        .await
        .expect("the guest's name reaches the host");
    assert_eq!(name["node_id"], guest_node.endpoint.id().to_string());
    assert!(host
        .peers()
        .await
        .iter()
        .any(|p| p.display_name.as_deref() == Some("Guest")));

    // With video built in, each side's test pattern reaches the other as
    // webview frames, and each sees its own self view.
    #[cfg(tutoring_video)]
    {
        let guest_id = guest_node.endpoint.id().to_string();
        let frame = host_events
            .wait_for("tutoring:video-frame", Duration::from_secs(20), |p| {
                p["node_id"] == guest_id.as_str()
            })
            .await
            .expect("the guest's video reaches the host");
        assert!(frame["width"].as_u64().unwrap() > 0);
        assert!(!frame["jpeg_b64"].as_str().unwrap().is_empty());
        host_events
            .wait_for("tutoring:video-frame", Duration::from_secs(10), |p| {
                p["node_id"] == "self"
            })
            .await
            .expect("the host sees its own camera");
        assert!(host.status().await.unwrap().video_enabled);
        assert!(!host.toggle_video(false).await.unwrap());
        host_events
            .wait_for("tutoring:peer-video-ended", Duration::from_secs(5), |p| {
                p["node_id"] == "self"
            })
            .await
            .expect("turning the camera off ends the self view");
        assert!(host.toggle_video(true).await.unwrap());
    }

    assert!(
        guest_events.payloads("tutoring:chat").is_empty()
            && guest_events.payloads("tutoring:transcript").is_empty(),
        "the sender never sees its own messages"
    );
    assert!(host.send_chat("x".repeat(2001)).await.is_err());
    assert!(guest.send_transcript("   ".into(), None).await.is_err());

    guest.shutdown().await.expect("guest cleanup");
    host.shutdown().await.expect("host cleanup");
}

#[tokio::test(flavor = "multi_thread")]
async fn leaving_a_live_room_does_not_wait_for_stuck_media_work() {
    let book = MemoryLookup::new();
    let node = Node::bind(&book).await;
    let records = MemoryRecords::migrated();
    let (manager, _) = manager();
    manager
        .create_room(
            request("Host", CallKind::Tutoring, None),
            node.network(),
            &records,
        )
        .await
        .expect("create");

    // A camera or encoder stuck in native code, owned by the session.
    let (release, released) = std::sync::mpsc::channel::<()>();
    let (started, running) = oneshot::channel::<()>();
    let tasks = manager.tasks().clone();
    let native = tokio::spawn(async move {
        let _ = tasks
            .run_blocking(move || {
                let _ = started.send(());
                let _ = released.recv();
            })
            .await;
    });
    running.await.expect("native work started");

    tokio::time::timeout(Duration::from_secs(10), manager.leave_room(&records))
        .await
        .expect("leave returns while media work is stuck")
        .expect("leave");
    assert!(manager.status().await.is_none());
    assert!(manager.send_chat("anyone?".into()).await.is_err());
    assert!(manager.toggle_audio(true).await.is_err());

    release.send(()).expect("finish native work");
    native.await.expect("native task");
    // Once the stuck work is done, the next call starts normally.
    tokio::time::timeout(
        Duration::from_secs(20),
        manager.create_room(
            request("Host", CallKind::Tutoring, None),
            node.network(),
            &records,
        ),
    )
    .await
    .expect("next room starts after cleanup")
    .expect("create again");
    manager.shutdown().await.expect("cleanup");
}

#[tokio::test(flavor = "multi_thread")]
async fn device_events_and_route_changes_reach_the_running_call() {
    let book = MemoryLookup::new();
    let node = Node::bind(&book).await;
    let records = MemoryRecords::migrated();
    let (manager, _) = manager();
    assert!(manager
        .set_audio_devices(Some("silence".into()), None)
        .await
        .is_err());

    manager
        .create_room(
            request("Host", CallKind::Tutoring, None),
            node.network(),
            &records,
        )
        .await
        .expect("create");
    manager
        .set_audio_devices(Some("silence".into()), Some("void".into()))
        .await
        .expect("route change");
    assert_eq!(
        manager.media().route().speaker_device_id.as_deref(),
        Some("void")
    );

    // A route change, an interruption and a trip to the background leave
    // the call running and answering.
    for event in [
        MediaEvent::AudioRouteChanged,
        MediaEvent::AudioInterrupted,
        MediaEvent::AudioResumed,
        MediaEvent::AppBackgrounded,
        MediaEvent::AppForegrounded,
    ] {
        manager.media().raise(event);
    }
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let status = manager.status().await.expect("still in the call");
    assert!(status.audio_enabled);
    assert_eq!(status.video_enabled, cfg!(tutoring_video));
    assert!(manager.toggle_audio(false).await.is_ok());

    manager.shutdown().await.expect("cleanup");
}

fn classroom_request(classroom: &str) -> RoomRequest {
    RoomRequest {
        classroom_call: Some(super::call_kind::ClassroomCallRow {
            channel_id: None,
            started_by: "stake_test1".into(),
        }),
        ..request("Teacher", CallKind::Classroom, Some(classroom))
    }
}

fn count(records: &MemoryRecords, table: &str) -> i64 {
    records.with(|db| {
        db.conn()
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn a_classroom_call_and_its_session_row_are_written_together_or_not_at_all() {
    let book = MemoryLookup::new();
    let node = Node::bind(&book).await;
    let records = MemoryRecords::migrated();
    let (manager, _) = manager();

    // The classroom does not exist, so its call row cannot be written: no
    // session row may be left behind as a phantom active call, and the room
    // is not left running.
    let refused = manager
        .create_room(
            classroom_request("no-such-classroom"),
            node.network(),
            &records,
        )
        .await
        .expect_err("the classroom row fails");
    assert!(refused.contains("FOREIGN KEY"), "{refused}");
    assert_eq!(count(&records, "tutoring_sessions"), 0);
    assert_eq!(count(&records, "classroom_calls"), 0);
    assert!(!manager.is_active().await);
    assert!(!manager.media().session_running());

    records.with(|db| {
        db.conn()
            .execute(
                "INSERT INTO classrooms (id, name, owner_address) VALUES ('c1', 'Algebra', 'addr')",
                [],
            )
            .unwrap()
    });
    let call = classroom_request("c1");
    let id = call.session_id.clone();
    let ticket = manager
        .create_room(call, node.network(), &records)
        .await
        .expect("classroom call starts");
    let (kind, context, status, _) = session_row(&records, &id);
    assert_eq!(
        (kind.as_str(), context.as_deref(), status.as_str()),
        ("classroom", Some("c1"), "active")
    );
    let stored: String = records.with(|db| {
        db.conn()
            .query_row(
                "SELECT ticket FROM classroom_calls WHERE id = ?1",
                [&id],
                |r| r.get(0),
            )
            .unwrap()
    });
    assert_eq!(stored, ticket);
    manager.shutdown().await.expect("cleanup");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_slow_leave_never_ends_the_media_of_the_call_that_follows() {
    let book = MemoryLookup::new();
    let node = Node::bind(&book).await;
    let records = MemoryRecords::migrated();
    let (manager, _) = manager();
    let manager = Arc::new(manager);
    manager
        .create_room(
            request("First", CallKind::Tutoring, None),
            node.network(),
            &records,
        )
        .await
        .expect("first call");

    // Hold the first call's cleanup open with native work that cannot be
    // aborted.
    let (release, released) = std::sync::mpsc::channel::<()>();
    let (started, running) = oneshot::channel::<()>();
    let tasks = manager.tasks().clone();
    let native = tokio::spawn(async move {
        let _ = tasks
            .run_blocking(move || {
                let _ = started.send(());
                let _ = released.recv();
            })
            .await;
    });
    running.await.expect("native work started");

    // Leaving gets as far as waiting on cleanup, and is not polled again
    // until the next call has started.
    let mut leave = Box::pin(manager.leave_room(&records));
    assert!(futures::poll!(&mut leave).is_pending());

    let next = {
        let manager = manager.clone();
        let records = records.clone();
        let network = node.network();
        tokio::spawn(async move {
            manager
                .create_room(
                    request("Second", CallKind::Tutoring, None),
                    network,
                    &records,
                )
                .await
        })
    };
    release.send(()).expect("finish native work");
    native.await.expect("native task");
    next.await.expect("join task").expect("second call starts");
    assert!(manager.media().session_running());

    // The first leave now finishes. It must not end the second call's media.
    leave.await.expect("first leave");
    assert!(
        manager.is_active().await,
        "the second call is still running"
    );
    assert!(
        manager.media().session_running(),
        "the late leave ended the next call's media"
    );

    manager.shutdown().await.expect("cleanup");
    assert!(!manager.media().session_running());
}
