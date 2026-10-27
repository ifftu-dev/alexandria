use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn active(directory: &std::path::Path) -> std::sync::Arc<AppState> {
    active_named(directory, "Learner").await
}

async fn active_named(directory: &std::path::Path, name: &str) -> std::sync::Arc<AppState> {
    let state = crate::profile::lifecycle_tests::state_in(directory);
    let paths = match state
        .profile_manager
        .list()
        .iter()
        .find(|p| p.display_name == name)
    {
        Some(profile) => state.profile_manager.paths_for(&profile.id),
        None => state
            .profile_manager
            .create(name, crate::profile::Avatar::default())
            .unwrap(),
    };
    let vault = if crate::crypto::keystore::Keystore::exists(&paths.vault_dir) {
        crate::crypto::keystore::Keystore::open(&paths.vault_dir, "test-password-123").unwrap()
    } else {
        // Android's keystore stores through `&mut self`.
        #[allow(unused_mut)]
        let mut vault =
            crate::crypto::keystore::Keystore::create(&paths.vault_dir, "test-password-123")
                .unwrap();
        vault.store_mnemonic(if name == "B" { "legal winner thank year wave sausage worth useful legal winner thank yellow" } else { "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about" }).unwrap();
        vault
    };
    let db = crate::db::Database::open_encrypted(&paths.db_path, &vault.derive_db_key()).unwrap();
    db.run_migrations().unwrap();
    *state.db.lock().unwrap() = Some(db);
    super::super::opens::prepare(&paths, &vault).unwrap();
    *state.keystore.lock().await = Some(vault);
    *state.active.write().unwrap() = Some(crate::ActiveProfile {
        id: paths.id.clone(),
        paths,
    });
    state
        .profile_operations
        .activate(async { Ok(()) }, async { Ok(()) })
        .await
        .unwrap();
    state
}

fn admitted(state: &AppState) -> crate::profile::scope::ProfileLease {
    state
        .profile_operations
        .admit(&state.profile_operations.session().unwrap())
        .unwrap()
}

#[tokio::test]
async fn registration_response_binds_receipt_to_the_registered_profile() {
    let directory = tempfile::tempdir().unwrap();
    let state = active(directory.path()).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    SettingsStore::set(
        state.db.lock().unwrap().as_ref().unwrap().conn(),
        keys::CLOUD_ORIGIN,
        origin.clone(),
    )
    .unwrap();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = [0; 8192];
        let count = socket.read(&mut request).await.unwrap();
        let request = String::from_utf8_lossy(&request[..count]);
        assert!(request.starts_with("PUT /api/push/devices/for/did:key:"));
        assert!(request.contains("x-alexandria-proof:"));
        let body = r#"{"recipient":"11111111-1111-4111-8111-111111111111"}"#;
        versioned_response(&mut socket, 200, body, wire_version(&request)).await;
    });
    let unavailable = register_with_token(&state, admitted(&state), async {
        Err("Native token unavailable".into())
    })
    .await
    .unwrap();
    assert_eq!(
        unavailable.last_error.as_deref(),
        Some("Native token unavailable")
    );
    let status = reconcile(
        &state,
        admitted(&state),
        Some("shared-platform-token".into()),
        None,
    )
    .await
    .unwrap();
    server.await.unwrap();
    assert!(status.registered);
    assert_eq!(status.last_error, None);
    SettingsStore::set(
        state.db.lock().unwrap().as_ref().unwrap().conn(),
        keys::USER_LANGUAGE,
        "FR_ca".to_string(),
    )
    .unwrap();
    let received = serde_json::json!({"data": {
        "recipient": "11111111-1111-4111-8111-111111111111",
        "kind": "offer_sent", "id": "22222222-2222-4222-8222-222222222222",
        "eventId": "offer_sent:22222222-2222-4222-8222-222222222222",
        "route": "/settings/organisations", "title": "Private provider name", "body": "did:key:private-provider-content"
    }});
    let recorded = receive(&state, admitted(&state), received)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        recorded.row.route.as_deref(),
        Some("/settings/organisations")
    );
    assert_eq!(recorded.unread, 1);
    assert_eq!(recorded.row.title, "Nouvelle offre");
    assert_eq!(
        recorded.row.body,
        "Une organisation vous a envoyé une offre."
    );
}

async fn request(socket: &mut tokio::net::TcpStream) -> (String, Value) {
    let mut bytes = Vec::new();
    loop {
        let mut buffer = [0; 4096];
        let count = socket.read(&mut buffer).await.unwrap();
        assert!(count > 0);
        bytes.extend_from_slice(&buffer[..count]);
        if let Some(end) = bytes.windows(4).position(|b| b == b"\r\n\r\n") {
            let header = String::from_utf8(bytes[..end].to_vec()).unwrap();
            let length = header
                .lines()
                .filter_map(|s| s.split_once(':'))
                .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
                .map(|(_, v)| v.trim().parse::<usize>().unwrap())
                .unwrap_or(0);
            if bytes.len() >= end + 4 + length {
                return (
                    header,
                    if length == 0 {
                        Value::Null
                    } else {
                        serde_json::from_slice(&bytes[end + 4..end + 4 + length]).unwrap()
                    },
                );
            }
        }
    }
}

async fn respond(socket: &mut tokio::net::TcpStream, code: u16, body: &str, header: &str) {
    versioned_response(socket, code, body, wire_version(header)).await;
}

#[tokio::test]
async fn offline_opt_out_survives_restart_and_rotation_until_original_device_is_deleted() {
    let directory = tempfile::tempdir().unwrap();
    let state = active(directory.path()).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    SettingsStore::set(
        state.db.lock().unwrap().as_ref().unwrap().conn(),
        keys::CLOUD_ORIGIN,
        origin,
    )
    .unwrap();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let (header, body) = request(&mut socket).await;
        let device = body["deviceId"].as_str().unwrap().to_string();
        respond(
            &mut socket,
            200,
            r#"{"recipient":"11111111-1111-4111-8111-111111111111"}"#,
            &header,
        )
        .await;
        let mut proofs = Vec::new();
        for code in [503, 204] {
            let (mut socket, _) = listener.accept().await.unwrap();
            let (header, _) = request(&mut socket).await;
            assert!(header.starts_with("DELETE /api/push/devices/for/did:key:"));
            assert!(header
                .lines()
                .next()
                .unwrap()
                .contains(&format!("/{device} HTTP/1.1")));
            proofs.push(
                header
                    .lines()
                    .find(|line| line.starts_with("x-alexandria-proof:"))
                    .unwrap()
                    .to_string(),
            );
            respond(&mut socket, code, "", &header).await;
        }
        assert_ne!(
            proofs[0], proofs[1],
            "every retry requires a fresh holder proof"
        );
    });
    assert!(
        reconcile(&state, admitted(&state), Some("old-token".into()), None)
            .await
            .unwrap()
            .registered
    );
    let off = reconcile(&state, admitted(&state), None, Some(false))
        .await
        .unwrap();
    assert!(!off.enabled && off.pending_cleanup && !off.registered);
    state.stop_active_profile().await.unwrap();
    drop(state);
    let reopened = active(directory.path()).await;
    let retried = reconcile(
        &reopened,
        admitted(&reopened),
        Some("rotated-token".into()),
        None,
    )
    .await
    .unwrap();
    assert!(!retried.enabled);
    assert!(
        !retried.pending_cleanup,
        "successful retry must acknowledge remote removal"
    );
    assert!(!retried.registered);
    server.await.unwrap();
}

fn offer(recipient: &str) -> Value {
    serde_json::json!({"data": {
        "recipient": recipient, "kind":"offer_sent", "id":"22222222-2222-4222-8222-222222222222",
        "eventId":"offer_sent:22222222-2222-4222-8222-222222222222",
        "route":"/settings/organisations", "title":"New offer", "body":"An organisation sent you an offer."
    }})
}

#[tokio::test]
async fn shared_token_receipt_and_held_tap_wait_for_matching_profile_across_restart() {
    let directory = tempfile::tempdir().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let mut devices = Vec::new();
        for recipient in [
            "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
            "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
        ] {
            let (mut socket, _) = listener.accept().await.unwrap();
            let (header, body) = request(&mut socket).await;
            assert_eq!(body["token"], "shared-token");
            devices.push(body["deviceId"].clone());
            respond(
                &mut socket,
                200,
                &serde_json::json!({"recipient":recipient}).to_string(),
                &header,
            )
            .await;
        }
        assert_ne!(devices[0], devices[1]);
    });
    let a = active_named(directory.path(), "A").await;
    let a_id = a.active_id().unwrap().to_string();
    SettingsStore::set(
        a.db.lock().unwrap().as_ref().unwrap().conn(),
        keys::CLOUD_ORIGIN,
        origin.clone(),
    )
    .unwrap();
    reconcile(&a, admitted(&a), Some("shared-token".into()), None)
        .await
        .unwrap();
    a.stop_active_profile().await.unwrap();
    drop(a);
    let b = active_named(directory.path(), "B").await;
    SettingsStore::set(
        b.db.lock().unwrap().as_ref().unwrap().conn(),
        keys::CLOUD_ORIGIN,
        origin,
    )
    .unwrap();
    reconcile(&b, admitted(&b), Some("shared-token".into()), None)
        .await
        .unwrap();
    server.await.unwrap();
    let payload = offer("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa");
    assert!(receive(&b, admitted(&b), payload.clone())
        .await
        .unwrap()
        .is_none());
    let tap = tapped(&b, payload.clone()).unwrap().unwrap();
    assert_eq!(tap.profile_id, a_id);
    assert_eq!(
        super::super::opens::pending(&b).await.unwrap().unwrap().id,
        tap.id
    );
    assert!(super::super::opens::consume(&b, &admitted(&b), &tap.id).is_err());
    b.stop_active_profile().await.unwrap();
    drop(b);
    let locked = crate::profile::lifecycle_tests::state_in(directory.path());
    assert_eq!(
        super::super::opens::pending(&locked)
            .await
            .unwrap()
            .unwrap()
            .id,
        tap.id
    );
    drop(locked);
    let a = active_named(directory.path(), "A").await;
    assert!(receive(&a, admitted(&a), payload.clone())
        .await
        .unwrap()
        .is_some());
    super::super::opens::consume(&a, &admitted(&a), &tap.id).unwrap();
    assert!(super::super::opens::pending(&a).await.unwrap().is_none());
    assert!(
        tapped(&a, payload).unwrap().is_none(),
        "duplicate native callback must not reopen a consumed intent"
    );
}

#[tokio::test]
async fn locking_while_native_token_is_pending_does_not_wait_or_register_in_a_new_session() {
    let directory = tempfile::tempdir().unwrap();
    let state = active(directory.path()).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    SettingsStore::set(
        state.db.lock().unwrap().as_ref().unwrap().conn(),
        keys::CLOUD_ORIGIN,
        format!("http://{}", listener.local_addr().unwrap()),
    )
    .unwrap();
    let old_session = state.profile_operations.session().unwrap();
    let lease = admitted(&state);
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (token_tx, token_rx) = tokio::sync::oneshot::channel();
    let task_state = state.clone();
    let task = tokio::spawn(async move {
        register_with_token(&task_state, lease, async {
            started_tx.send(()).unwrap();
            token_rx.await.map_err(|e| e.to_string())
        })
        .await
    });
    started_rx.await.unwrap();
    let cleanup = state.clone();
    tokio::time::timeout(
        std::time::Duration::from_secs(3),
        state
            .profile_operations
            .lock(async move { cleanup.stop_active_profile().await }),
    )
    .await
    .expect("native SDK work must not retain a profile lease")
    .unwrap();
    // Reopen even the same identity: a new session must reject the old work.
    let paths = state
        .profile_manager
        .paths_for(&state.profile_manager.list()[0].id);
    let vault =
        crate::crypto::keystore::Keystore::open(&paths.vault_dir, "test-password-123").unwrap();
    let db = crate::db::Database::open_encrypted(&paths.db_path, &vault.derive_db_key()).unwrap();
    *state.db.lock().unwrap() = Some(db);
    super::super::opens::prepare(&paths, &vault).unwrap();
    *state.keystore.lock().await = Some(vault);
    *state.active.write().unwrap() = Some(crate::ActiveProfile {
        id: paths.id.clone(),
        paths,
    });
    state
        .profile_operations
        .activate(async { Ok(()) }, async { Ok(()) })
        .await
        .unwrap();
    assert_ne!(state.profile_operations.session().unwrap(), old_session);
    token_tx.send("late-token".to_string()).unwrap();
    assert!(task.await.unwrap().is_err());
    assert!(!status(&state, admitted(&state)).await.unwrap().registered);
}

#[tokio::test]
async fn a_put_response_after_preference_changes_cannot_publish_a_recipient() {
    let directory = tempfile::tempdir().unwrap();
    let state = active(directory.path()).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    SettingsStore::set(
        state.db.lock().unwrap().as_ref().unwrap().conn(),
        keys::CLOUD_ORIGIN,
        format!("http://{}", listener.local_addr().unwrap()),
    )
    .unwrap();
    let (sent_tx, sent_rx) = tokio::sync::oneshot::channel();
    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let (header, _) = request(&mut socket).await;
        sent_tx.send(()).unwrap();
        reply_rx.await.unwrap();
        respond(
            &mut socket,
            200,
            r#"{"recipient":"11111111-1111-4111-8111-111111111111"}"#,
            &header,
        )
        .await;
    });
    let task_state = state.clone();
    let task = tokio::spawn(async move {
        reconcile(
            &task_state,
            admitted(&task_state),
            Some("token".into()),
            None,
        )
        .await
    });
    sent_rx.await.unwrap();
    // The existing settings API may persist a preference while HTTP is pending.
    SettingsStore::set(
        state.db.lock().unwrap().as_ref().unwrap().conn(),
        keys::NOTIFICATIONS_PUSH_ENABLED,
        false,
    )
    .unwrap();
    reply_tx.send(()).unwrap();
    let status = task.await.unwrap().unwrap();
    assert!(!status.enabled && status.pending_cleanup);
    assert!(
        tapped(&state, offer("11111111-1111-4111-8111-111111111111"))
            .unwrap()
            .is_none()
    );
    server.await.unwrap();
}

#[tokio::test]
async fn interrupted_put_is_cleaned_at_original_origin_after_restart_and_origin_change() {
    let directory = tempfile::tempdir().unwrap();
    let state = active(directory.path()).await;
    let old = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let new = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let new_origin = format!("http://{}", new.local_addr().unwrap());
    SettingsStore::set(
        state.db.lock().unwrap().as_ref().unwrap().conn(),
        keys::CLOUD_ORIGIN,
        format!("http://{}", old.local_addr().unwrap()),
    )
    .unwrap();
    let (sent_tx, sent_rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut socket, _) = old.accept().await.unwrap();
        let (_, body) = request(&mut socket).await;
        let device = body["deviceId"].as_str().unwrap().to_string();
        sent_tx.send(()).unwrap();
        // Cloud has committed the PUT; its response is lost with the connection.
        let (mut cleanup, _) = old.accept().await.unwrap();
        let (header, _) = request(&mut cleanup).await;
        assert!(header.starts_with("DELETE /api/push/devices/for/did:key:"));
        assert!(header
            .lines()
            .next()
            .unwrap()
            .contains(&format!("/{device} HTTP/1.1")));
        respond(&mut cleanup, 204, "", &header).await;
    });
    let task_state = state.clone();
    let task = tokio::spawn(async move {
        reconcile(
            &task_state,
            admitted(&task_state),
            Some("old-token".into()),
            None,
        )
        .await
    });
    sent_rx.await.unwrap();
    let cleanup = state.clone();
    tokio::time::timeout(
        std::time::Duration::from_secs(3),
        state
            .profile_operations
            .lock(async move { cleanup.stop_active_profile().await }),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(task.await.unwrap().is_err());
    drop(state);
    let reopened = active(directory.path()).await;
    assert!(
        status(&reopened, admitted(&reopened))
            .await
            .unwrap()
            .pending_registration
    );
    SettingsStore::set(
        reopened.db.lock().unwrap().as_ref().unwrap().conn(),
        keys::CLOUD_ORIGIN,
        new_origin,
    )
    .unwrap();
    let off = reconcile(
        &reopened,
        admitted(&reopened),
        Some("rotated-token".into()),
        Some(false),
    )
    .await
    .unwrap();
    assert!(!off.enabled && !off.pending_cleanup && !off.registered);
    server.await.unwrap();
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(50), new.accept())
            .await
            .is_err(),
        "cleanup must not be redirected to the new origin"
    );
}

#[tokio::test]
async fn legacy_response_and_rotated_bindings_fail_closed_and_wire_routes_are_exact() {
    let directory = tempfile::tempdir().unwrap();
    let state = active(directory.path()).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    SettingsStore::set(
        state.db.lock().unwrap().as_ref().unwrap().conn(),
        keys::CLOUD_ORIGIN,
        origin.clone(),
    )
    .unwrap();
    let server = tokio::spawn(async move {
        let mut devices = Vec::new();
        for (code, body) in [
            (204, ""),
            (
                200,
                r#"{"recipient":"aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"}"#,
            ),
            (
                200,
                r#"{"recipient":"bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb"}"#,
            ),
        ] {
            let (mut socket, _) = listener.accept().await.unwrap();
            let (header, request) = request(&mut socket).await;
            devices.push(request["deviceId"].clone());
            respond(&mut socket, code, body, &header).await;
        }
        assert!(
            devices.windows(2).all(|pair| pair[0] == pair[1]),
            "PUT retry and rotation preserve device identity"
        );
    });
    let unresolved = reconcile(
        &state,
        admitted(&state),
        Some("original-token".into()),
        None,
    )
    .await
    .unwrap();
    assert!(!unresolved.registered && unresolved.pending_registration);
    assert!(
        tapped(&state, offer("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"))
            .unwrap()
            .is_none()
    );
    state.stop_active_profile().await.unwrap();
    drop(state);
    let state = active(directory.path()).await;
    assert!(
        reconcile(&state, admitted(&state), Some("rotated-token".into()), None)
            .await
            .unwrap()
            .registered
    );
    assert!(
        reconcile(&state, admitted(&state), Some("rotated-again".into()), None)
            .await
            .unwrap()
            .registered
    );
    server.await.unwrap();
    assert!(
        tapped(&state, offer("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"))
            .unwrap()
            .is_none()
    );
    let sidecar =
        std::fs::read_to_string(directory.path().join("notification_recipients.json")).unwrap();
    assert!(
        !sidecar.contains("token") && !sidecar.contains("did:") && !sidecar.contains("Learner")
    );
    let entries: Value = serde_json::from_str(&sidecar).unwrap();
    assert_eq!(
        entries[0].as_object().unwrap().len(),
        4,
        "locked routing stores only recipient, profile, network and trusted origin"
    );
    let valid = offer("bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb");
    for damage in ["network", "profile", "ambiguous"] {
        let mut damaged = entries.clone();
        match damage {
            "network" => damaged[0]["network_id"] = "another-network".into(),
            "profile" => damaged[0]["profile_id"] = "cccccccc-cccc-4ccc-8ccc-cccccccccccc".into(),
            _ => {
                let duplicate = damaged[0].clone();
                damaged.as_array_mut().unwrap().push(duplicate);
            }
        }
        std::fs::write(
            directory.path().join("notification_recipients.json"),
            serde_json::to_vec(&damaged).unwrap(),
        )
        .unwrap();
        assert!(tapped(&state, valid.clone()).unwrap().is_none());
        assert!(receive(&state, admitted(&state), valid.clone())
            .await
            .unwrap()
            .is_none());
    }
    std::fs::write(
        directory.path().join("notification_recipients.json"),
        &sidecar,
    )
    .unwrap();

    let mut legacy = valid.clone();
    legacy["data"].as_object_mut().unwrap().remove("recipient");
    let mut malformed = vec![legacy, offer("cccccccc-cccc-4ccc-8ccc-cccccccccccc")];
    for (key, value) in [
        (
            "eventId",
            "interview_invited:22222222-2222-4222-8222-222222222222",
        ),
        ("id", "legacy-id"),
        ("route", "https://foreign.example/"),
        (
            "route",
            "/settings/organisations?cloud=https://foreign.example",
        ),
        ("recipient", "BBBBBBBB-BBBB-4BBB-8BBB-BBBBBBBBBBBB"),
        ("cloudOrigin", "https://foreign.example"),
    ] {
        let mut payload = valid.clone();
        payload["data"][key] = Value::String(value.into());
        malformed.push(payload);
    }
    for payload in malformed {
        assert!(receive(&state, admitted(&state), payload.clone())
            .await
            .unwrap()
            .is_none());
        assert!(tapped(&state, payload).unwrap().is_none());
    }
    let mut room = valid;
    room["data"]["kind"] = "interview_room_opened".into();
    room["data"]["eventId"] = "interview_room_opened:22222222-2222-4222-8222-222222222222:33333333-3333-4333-8333-333333333333".into();
    room["data"]["route"] = "/interview-room/22222222-2222-4222-8222-222222222222".into();
    let recorded = receive(&state, admitted(&state), room.clone())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(recorded.unread, 1);
    assert_eq!(
        recorded.row.route.as_deref(),
        Some("/interview-room/22222222-2222-4222-8222-222222222222")
    );
    assert_eq!(
        recorded.row.payload.as_ref().unwrap()["cloudOrigin"],
        origin
    );
    assert!(receive(&state, admitted(&state), room)
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn local_banner_tap_survives_lock_and_rejects_legacy_profileless_extras() {
    let directory = tempfile::tempdir().unwrap();
    let state = active(directory.path()).await;
    let profile = state.active_id().unwrap().to_string();
    let payload = serde_json::json!({"local": true, "data": {"__EXTRA__": {
        "profileId": profile, "networkId": recipients::network().unwrap(),
        "notificationId": "11111111-1111-4111-8111-111111111111", "route": "/credentials/credential"
    }}});
    state.stop_active_profile().await.unwrap();
    drop(state);
    let locked = crate::profile::lifecycle_tests::state_in(directory.path());
    let intent = tapped(&locked, payload.clone()).unwrap().unwrap();
    assert_eq!(intent.profile_id, profile);
    assert_eq!(intent.recipient, None);
    let held = super::super::opens::pending(&locked)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(held.id, intent.id);
    assert!(held.route.is_empty());
    let mut legacy = payload;
    legacy["data"]["__EXTRA__"]
        .as_object_mut()
        .unwrap()
        .remove("profileId");
    assert!(tapped(&locked, legacy).unwrap().is_none());
}

#[tokio::test]
async fn locked_pending_open_ipc_peeks_without_consuming_and_requires_session_for_ack() {
    use tauri::test::{get_ipc_response, mock_builder, mock_context, noop_assets};
    let directory = tempfile::tempdir().unwrap();
    let state = active(directory.path()).await;
    let profile = state.profile_manager.paths_for(&state.active_id().unwrap());
    state.stop_active_profile().await.unwrap();
    let mut intent = tapped(
        &state,
        serde_json::json!({"local": true, "data": {"__EXTRA__": {
            "profileId": profile.id.as_str(), "networkId": recipients::network().unwrap(),
            "notificationId": "11111111-1111-4111-8111-111111111111", "route": "/notifications"
        }}}),
    )
    .unwrap()
    .unwrap();
    intent.route.clear();
    let state = std::sync::Arc::try_unwrap(state).unwrap_or_else(|_| panic!("unique state"));
    let app = mock_builder()
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            crate::commands::notifications::notifications_pending_open,
            crate::commands::notifications::notifications_consume_open
        ])
        .build(mock_context(noop_assets()))
        .unwrap();
    let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    for command in [
        "notifications_pending_open",
        "notifications_pending_open",
        "notifications_consume_open",
    ] {
        let webview = webview.clone();
        let id = intent.id.clone();
        let response = tokio::task::spawn_blocking(move || {
            get_ipc_response(
                &webview,
                tauri::webview::InvokeRequest {
                    cmd: command.into(),
                    callback: tauri::ipc::CallbackFn(0),
                    error: tauri::ipc::CallbackFn(1),
                    url: "tauri://localhost".parse().unwrap(),
                    body: tauri::ipc::InvokeBody::Json(serde_json::json!({"id": id})),
                    headers: Default::default(),
                    invoke_key: tauri::test::INVOKE_KEY.to_string(),
                },
            )
        })
        .await
        .unwrap();
        if command == "notifications_pending_open" {
            assert_eq!(
                response
                    .unwrap()
                    .deserialize::<super::super::opens::NotificationOpen>()
                    .unwrap(),
                intent
            );
        } else {
            assert_eq!(
                response.unwrap_err(),
                serde_json::json!("profile session header is required")
            );
        }
    }
}

#[tokio::test]
async fn a_native_token_result_cannot_overwrite_a_newer_rotation_in_the_same_session() {
    let directory = tempfile::tempdir().unwrap();
    let state = active(directory.path()).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    SettingsStore::set(
        state.db.lock().unwrap().as_ref().unwrap().conn(),
        keys::CLOUD_ORIGIN,
        format!("http://{}", listener.local_addr().unwrap()),
    )
    .unwrap();
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (token_tx, token_rx) = tokio::sync::oneshot::channel();
    let task_state = state.clone();
    let task = tokio::spawn(async move {
        register_with_token(&task_state, admitted(&task_state), async {
            started_tx.send(()).unwrap();
            token_rx.await.map_err(|e| e.to_string())
        })
        .await
    });
    started_rx.await.unwrap();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let (header, body) = request(&mut socket).await;
        assert_eq!(body["token"], "new-token");
        respond(
            &mut socket,
            200,
            r#"{"recipient":"11111111-1111-4111-8111-111111111111"}"#,
            &header,
        )
        .await;
        listener
    });
    reconcile(&state, admitted(&state), Some("new-token".into()), None)
        .await
        .unwrap();
    token_tx.send("stale-token".into()).unwrap();
    assert!(task.await.unwrap().unwrap().registered);
    let listener = server.await.unwrap();
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(50), listener.accept())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn recipient_push_and_authenticated_pull_share_room_opening_identity() {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    let directory = tempfile::tempdir().unwrap();
    let state = active(directory.path()).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    SettingsStore::set(
        state.db.lock().unwrap().as_ref().unwrap().conn(),
        keys::CLOUD_ORIGIN,
        origin.clone(),
    )
    .unwrap();
    let next_opening = Arc::new(AtomicBool::new(false));
    let server_next = next_opening.clone();
    let server = tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            let (header, _) = request(&mut socket).await;
            assert!(header.contains("x-alexandria-proof:"));
            let body = if header.starts_with("PUT ") {
                serde_json::json!({"recipient":"aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"})
            } else if header.starts_with("GET /api/interviews/rooms/for/") {
                let opening = if server_next.load(Ordering::SeqCst) {
                    "44444444-4444-4444-8444-444444444444"
                } else {
                    "33333333-3333-4333-8333-333333333333"
                };
                serde_json::json!([{
                    "id":"22222222-2222-4222-8222-222222222222", "openingId":opening,
                    "kind":"interview_room_opened", "eventId":format!("interview_room_opened:22222222-2222-4222-8222-222222222222:{opening}"),
                    "route":"/interview-room/22222222-2222-4222-8222-222222222222",
                    "openedAt":"2026-10-27T00:00:00Z", "expiresAt":"2099-01-01T00:00:00Z"
                }])
            } else {
                serde_json::json!([])
            };
            respond(&mut socket, 200, &body.to_string(), &header).await;
        }
    });
    reconcile(
        &state,
        admitted(&state),
        Some("shared-device-token".into()),
        None,
    )
    .await
    .unwrap();
    let mut push = offer("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa");
    push["data"]["kind"] = "interview_room_opened".into();
    push["data"]["eventId"] = "interview_room_opened:22222222-2222-4222-8222-222222222222:33333333-3333-4333-8333-333333333333".into();
    push["data"]["route"] = "/interview-room/22222222-2222-4222-8222-222222222222".into();
    let received = receive(&state, admitted(&state), push.clone())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        received.row.payload.as_ref().unwrap()["cloudOrigin"],
        origin
    );
    assert_eq!(
        received.row.payload.as_ref().unwrap()["eventId"],
        push["data"]["eventId"]
    );
    assert_eq!(
        received.row.route.as_deref(),
        Some("/interview-room/22222222-2222-4222-8222-222222222222")
    );
    let (key, did) = issuer_key(&state).await.unwrap();
    for clear in [false, true] {
        let items = super::super::cloud_pull::fetch(&origin, &key, did.as_str()).await;
        assert_eq!(items.len(), 1);
        let fresh = state
            .db_executor
            .execute(
                DatabaseWorkload::Background,
                admitted(&state),
                "test.notification_recovery",
                move |db| {
                    if clear {
                        super::super::clear(db.conn())?;
                    }
                    Ok(super::super::cloud_pull::unreported(db.conn(), &items).len())
                },
            )
            .await
            .unwrap();
        assert_eq!(
            fresh, 0,
            "push then pull, including after clear, must stay deduplicated"
        );
        assert!(receive(&state, admitted(&state), push.clone())
            .await
            .unwrap()
            .is_none());
    }
    next_opening.store(true, Ordering::SeqCst);
    let items = super::super::cloud_pull::fetch(&origin, &key, did.as_str()).await;
    let recorded = state
        .db_executor
        .execute(
            DatabaseWorkload::Background,
            admitted(&state),
            "test.notification_recovery",
            move |db| {
                let fresh = super::super::cloud_pull::unreported(db.conn(), &items);
                assert_eq!(fresh.len(), 1, "a genuine new room opening is a new event");
                super::super::record_if_enabled(db.conn(), &fresh[0].notification().unwrap())
            },
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(recorded.unread, 1);
    push["data"]["eventId"] = "interview_room_opened:22222222-2222-4222-8222-222222222222:44444444-4444-4444-8444-444444444444".into();
    assert!(
        receive(&state, admitted(&state), push.clone())
            .await
            .unwrap()
            .is_none(),
        "pull then push must also deduplicate"
    );
    let held = tapped(&state, push).unwrap().unwrap();
    assert_eq!(
        held.route,
        format!(
            "/interview-room/22222222-2222-4222-8222-222222222222?cloud={}",
            url::form_urlencoded::byte_serialize(origin.as_bytes()).collect::<String>()
        )
    );
    server.abort();
}

#[tokio::test]
async fn held_activity_is_opaque_on_disk_and_opens_only_after_matching_unlock() {
    let directory = tempfile::tempdir().unwrap();
    let state = active(directory.path()).await;
    let profile = state.active_id().unwrap().to_string();
    let route =
        "/interview-room/22222222-2222-4222-8222-222222222222?cloud=https%3A%2F%2Fprivate.example";
    let payload = serde_json::json!({"local": true, "data": {"__EXTRA__": {
        "profileId": profile, "networkId": recipients::network().unwrap(),
        "notificationId": "11111111-1111-4111-8111-111111111111", "route": route
    }}});
    let intent = tapped(&state, payload.clone()).unwrap().unwrap();
    let bytes = std::fs::read(directory.path().join("notification_open.json")).unwrap();
    let disk = String::from_utf8_lossy(&bytes);
    assert!(
        !disk.contains("22222222-2222-4222-8222-222222222222"),
        "destination must be protected at rest"
    );
    assert!(
        !disk.contains("11111111-1111-4111-8111-111111111111"),
        "event identity must be opaque at rest"
    );
    assert!(!disk.contains("private.example"));
    state.stop_active_profile().await.unwrap();
    drop(state);
    let locked = crate::profile::lifecycle_tests::state_in(directory.path());
    let held = super::super::opens::pending(&locked)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(held.profile_id, profile);
    assert!(
        held.route.is_empty(),
        "locked IPC exposes only routing metadata"
    );
    drop(locked);
    let state = active(directory.path()).await;
    assert_eq!(
        super::super::opens::pending(&state)
            .await
            .unwrap()
            .unwrap()
            .route,
        route
    );
    super::super::opens::consume(&state, &admitted(&state), &intent.id).unwrap();
    assert!(tapped(&state, payload).unwrap().is_none());
    let disk = std::fs::read_to_string(directory.path().join("notification_open.json")).unwrap();
    assert!(!disk.contains("11111111-1111-4111-8111-111111111111"));
}

fn wire_version(header: &str) -> i64 {
    header
        .lines()
        .filter_map(|line| line.split_once(':'))
        .find(|(key, _)| key.eq_ignore_ascii_case("x-alexandria-push-version"))
        .and_then(|(_, value)| value.trim().parse().ok())
        .unwrap_or(0)
}

async fn versioned_response(
    socket: &mut tokio::net::TcpStream,
    code: u16,
    body: &str,
    version: i64,
) {
    socket.write_all(format!("HTTP/1.1 {code} Test\r\nx-alexandria-push-version: {version}\r\nx-alexandria-push-protocol: versioned-v1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
}

#[tokio::test]
async fn deletion_ack_fences_a_put_that_executes_later_and_reenable_uses_fresh_identity() {
    let directory = tempfile::tempdir().unwrap();
    let state = active(directory.path()).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    SettingsStore::set(
        state.db.lock().unwrap().as_ref().unwrap().conn(),
        keys::CLOUD_ORIGIN,
        format!("http://{}", listener.local_addr().unwrap()),
    )
    .unwrap();
    let (sent_tx, sent_rx) = tokio::sync::oneshot::channel();
    let (execute_tx, execute_rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut delayed_put, _) = listener.accept().await.unwrap();
        let (put, body) = request(&mut delayed_put).await;
        let device = body["deviceId"].clone();
        sent_tx.send(()).unwrap();
        // The original PUT has arrived but has NOT executed. The DELETE
        // commits first against an absent registration and returns its barrier.
        let (mut delete, _) = listener.accept().await.unwrap();
        let (delete_header, _) = request(&mut delete).await;
        let revoked_version = wire_version(&delete_header);
        versioned_response(&mut delete, 204, "", revoked_version).await;
        execute_rx.await.unwrap(); // caller has observed successful opt-out
        let resurrected = wire_version(&put) >= revoked_version;
        let (mut reenable, _) = listener.accept().await.unwrap();
        let (next, next_body) = request(&mut reenable).await;
        versioned_response(
            &mut reenable,
            200,
            r#"{"recipient":"11111111-1111-4111-8111-111111111111"}"#,
            wire_version(&next),
        )
        .await;
        (
            wire_version(&put),
            revoked_version,
            resurrected,
            device,
            wire_version(&next),
            next_body["deviceId"].clone(),
        )
    });
    let task_state = state.clone();
    let put = tokio::spawn(async move {
        reconcile(
            &task_state,
            admitted(&task_state),
            Some("old-token".into()),
            None,
        )
        .await
    });
    sent_rx.await.unwrap();
    let cleanup = state.clone();
    tokio::time::timeout(
        std::time::Duration::from_secs(3),
        state
            .profile_operations
            .lock(async move { cleanup.stop_active_profile().await }),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(put.await.unwrap().is_err());
    drop(state);
    let state = active(directory.path()).await;
    let off = reconcile(&state, admitted(&state), None, Some(false))
        .await
        .unwrap();
    assert!(!off.enabled && !off.pending_cleanup);
    execute_tx.send(()).unwrap();
    let on = reconcile(
        &state,
        admitted(&state),
        Some("new-token".into()),
        Some(true),
    )
    .await
    .unwrap();
    assert!(on.registered);
    let (put_version, delete_version, resurrected, old_device, next_version, new_device) =
        server.await.unwrap();
    assert_eq!(put_version, 1);
    assert_eq!(delete_version, 2);
    assert!(
        !resurrected,
        "a delayed PUT must not undo acknowledged opt-out"
    );
    assert_ne!(old_device, new_device);
    assert_eq!(next_version, 1);
}

#[tokio::test]
async fn unchanged_pending_put_retries_the_same_version_and_body_after_restart() {
    let directory = tempfile::tempdir().unwrap();
    let state = active(directory.path()).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    SettingsStore::set(
        state.db.lock().unwrap().as_ref().unwrap().conn(),
        keys::CLOUD_ORIGIN,
        format!("http://{}", listener.local_addr().unwrap()),
    )
    .unwrap();
    let server = tokio::spawn(async move {
        let mut requests = Vec::new();
        for code in [503, 200, 200] {
            let (mut socket, _) = listener.accept().await.unwrap();
            let (header, body) = request(&mut socket).await;
            let version = wire_version(&header);
            requests.push((version, body));
            versioned_response(
                &mut socket,
                code,
                r#"{"recipient":"11111111-1111-4111-8111-111111111111"}"#,
                version,
            )
            .await;
        }
        requests
    });
    assert!(
        reconcile(&state, admitted(&state), Some("token-A".into()), None)
            .await
            .unwrap()
            .pending_registration
    );
    state.stop_active_profile().await.unwrap();
    drop(state);
    let state = active(directory.path()).await;
    assert!(
        reconcile(&state, admitted(&state), Some("token-A".into()), None)
            .await
            .unwrap()
            .registered
    );
    assert!(
        reconcile(&state, admitted(&state), Some("token-B".into()), None)
            .await
            .unwrap()
            .registered
    );
    let requests = server.await.unwrap();
    assert_eq!(
        requests[0], requests[1],
        "an unchanged retry must reuse its persisted mutation version and payload"
    );
    assert_eq!(requests[2].0, 2);
    assert_eq!(requests[2].1["token"], "token-B");
    assert_eq!(requests[0].1["deviceId"], requests[2].1["deviceId"]);
}

#[tokio::test]
async fn unversioned_or_mismatched_delete_ack_keeps_cleanup_identity_across_restart() {
    let directory = tempfile::tempdir().unwrap();
    let state = active(directory.path()).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    SettingsStore::set(
        state.db.lock().unwrap().as_ref().unwrap().conn(),
        keys::CLOUD_ORIGIN,
        format!("http://{}", listener.local_addr().unwrap()),
    )
    .unwrap();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let (put, body) = request(&mut socket).await;
        versioned_response(
            &mut socket,
            200,
            r#"{"recipient":"11111111-1111-4111-8111-111111111111"}"#,
            wire_version(&put),
        )
        .await;
        let mut deletions = Vec::new();
        for response in 0..3 {
            let (mut socket, _) = listener.accept().await.unwrap();
            let (header, _) = request(&mut socket).await;
            deletions.push((
                header.lines().next().unwrap().to_string(),
                wire_version(&header),
            ));
            if response == 0 {
                socket.write_all(b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await.unwrap();
            } else {
                versioned_response(
                    &mut socket,
                    204,
                    "",
                    if response == 1 {
                        999
                    } else {
                        wire_version(&header)
                    },
                )
                .await;
            }
        }
        (body["deviceId"].as_str().unwrap().to_string(), deletions)
    });
    assert!(
        reconcile(&state, admitted(&state), Some("token".into()), None)
            .await
            .unwrap()
            .registered
    );
    let off = reconcile(&state, admitted(&state), None, Some(false))
        .await
        .unwrap();
    assert!(!off.enabled && off.pending_cleanup);
    state.stop_active_profile().await.unwrap();
    drop(state);
    let state = active(directory.path()).await;
    let retry = reconcile(&state, admitted(&state), Some("rotated-token".into()), None)
        .await
        .unwrap();
    assert!(!retry.enabled && retry.pending_cleanup);
    let clean = reconcile(&state, admitted(&state), None, None)
        .await
        .unwrap();
    assert!(!clean.enabled && !clean.pending_cleanup);
    let (device, deletions) = server.await.unwrap();
    assert!(deletions
        .iter()
        .all(|(path, version)| path.contains(&device) && *version == 2));
}

#[tokio::test]
async fn locked_startup_removes_incompatible_plaintext_prototype_activity() {
    let directory = tempfile::tempdir().unwrap();
    let state = crate::profile::lifecycle_tests::state_in(directory.path());
    let path = directory.path().join("notification_open.json");
    std::fs::write(&path, r#"{"pending":null,"consumed":["recipient:interview_room_opened:private-interview:opening"]}"#).unwrap();
    assert!(super::super::opens::pending(&state)
        .await
        .unwrap()
        .is_none());
    assert!(
        !path.exists(),
        "unversioned plaintext prototype activity must not remain on disk"
    );
}
