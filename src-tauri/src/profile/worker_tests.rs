use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::cardano::blockfrost::BlockfrostClient;
use crate::cardano::submission::{self, Journal, Operation, SubmissionStatus};
use crate::cardano::tx_builder;
use crate::crypto::keystore::Keystore;
use crate::db::executor::DatabaseWorkload;
use crate::db::Database;
use crate::{ActiveProfile, AppState};

const OPERATION: Operation<'static> = Operation {
    kind: "profile_lock_test",
    id: "original",
};
const CONTEXT: &str = r#"{"version":1}"#;
const PASSWORD: &str = "test-password-123";

// Install real vault/database/content-key resources without starting public
// discovery, plugin installation or hardware. Teardown uses the production
// AppState::stop_active_profile and the normal transition/admission owner.
async fn install_resources(state: &AppState, paths: super::ProfilePaths) -> Result<(), String> {
    let keystore = if Keystore::exists(&paths.vault_dir) {
        Keystore::open(&paths.vault_dir, PASSWORD)
    } else {
        Keystore::create(&paths.vault_dir, PASSWORD)
    }
    .map_err(|e| e.to_string())?;
    let key = zeroize::Zeroizing::new(keystore.derive_db_key());
    let database = Database::open_encrypted(&paths.db_path, &key).map_err(|e| e.to_string())?;
    database.run_migrations().map_err(|e| e.to_string())?;
    state
        .content_node
        .set_content_key(keystore.derive_content_key())
        .await;
    *state.db.lock().unwrap() = Some(database);
    *state.keystore.lock().await = Some(keystore);
    *state.active.write().unwrap() = Some(ActiveProfile {
        id: paths.id.clone(),
        paths,
    });
    Ok(())
}

async fn activate(state: &Arc<AppState>, paths: super::ProfilePaths) {
    let startup = state.clone();
    let cleanup = state.clone();
    state
        .profile_operations
        .activate(
            async move { install_resources(&startup, paths).await },
            async move { cleanup.stop_active_profile().await },
        )
        .await
        .unwrap();
}

async fn lock(state: &Arc<AppState>) {
    let cleanup = state.clone();
    tokio::time::timeout(
        Duration::from_secs(5),
        state
            .profile_operations
            .lock(async move { cleanup.stop_active_profile().await }),
    )
    .await
    .expect("profile cleanup stalled")
    .unwrap();
    assert!(state.db.lock().unwrap().is_none());
    assert!(state.keystore.lock().await.is_none());
    assert!(state.content_node.content_key().await.is_none());
    assert!(state.active_id().is_none());
    assert!(!state.profile_operations.cleanup_required().await);
}

fn signed_transaction(fee: u64) -> Vec<u8> {
    use pallas_addresses::Address;
    use pallas_crypto::{hash::Hash, key::ed25519::SecretKey};
    use pallas_txbuilder::{BuildConway, Input, Output, StagingTransaction};
    use pallas_wallet::PrivateKey;

    let transaction = StagingTransaction::new()
        .input(Input::new(Hash::from([0x42; 32]), 0))
        .output(Output::new(Address::from_bech32(
            "addr_test1qz2fxv2umyhttkxyxp8x0dlpdt3k6cwng5pxj3jhsydzer3jcu5d8ps7zex2k2xt3uqxgjqnnj83ws8lhrn648jjxtwq2ytjqp"
        ).unwrap(), 2_000_000))
        .fee(fee).network_id(0).build_conway_raw().unwrap();
    tx_builder::sign_raw_tx(
        &transaction.tx_bytes.0,
        &PrivateKey::Normal(SecretKey::from([7; 32])),
    )
    .unwrap()
}

async fn read_request(socket: &mut tokio::net::TcpStream) -> (String, Vec<u8>) {
    let mut bytes = Vec::new();
    loop {
        let mut chunk = [0; 1024];
        let count = socket.read(&mut chunk).await.unwrap();
        assert_ne!(count, 0, "incomplete request");
        bytes.extend_from_slice(&chunk[..count]);
        assert!(bytes.len() < 64 * 1024);
        if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            let header = String::from_utf8(bytes[..end].to_vec()).unwrap();
            let length = header
                .lines()
                .filter_map(|line| line.split_once(':'))
                .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                .map(|(_, value)| value.trim().parse::<usize>().unwrap())
                .unwrap_or(0);
            if bytes.len() >= end + 4 + length {
                return (header, bytes[end + 4..end + 4 + length].to_vec());
            }
        }
    }
}

#[tokio::test]
async fn locking_stalled_post_cleans_profile_and_recovers_original_after_switch() {
    let directory = tempfile::tempdir().unwrap();
    let state = super::lifecycle_tests::state_in(directory.path());
    let original_paths = state
        .profile_manager
        .create("Original", super::Avatar::default())
        .unwrap();
    let next_paths = state
        .profile_manager
        .create("Next", super::Avatar::default())
        .unwrap();
    activate(&state, original_paths.clone()).await;
    let original_session = state.profile_operations.session().unwrap();
    let lease = state.profile_operations.admit(&original_session).unwrap();
    let bytes = signed_transaction(200_000);
    let expected = tx_builder::compute_tx_hash(&bytes).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let client = BlockfrostClient::with_base_url(
        "test".into(),
        format!("http://{}", listener.local_addr().unwrap()),
    )
    .unwrap();
    let (posted_tx, posted_rx) = tokio::sync::oneshot::channel();
    let (late_tx, late_rx) = tokio::sync::oneshot::channel();
    let (sent_tx, sent_rx) = tokio::sync::oneshot::channel();
    let server_hash = expected.clone();
    let server = tokio::spawn(async move {
        let (mut first, _) = listener.accept().await.unwrap();
        let (header, body) = read_request(&mut first).await;
        assert!(header.starts_with("POST /tx/submit HTTP/1.1"));
        posted_tx.send(body).unwrap();
        late_rx.await.unwrap();
        let body = serde_json::to_string(&server_hash).unwrap();
        // The client may already have closed the socket. Either outcome must
        // leave the next profile untouched by the abandoned response.
        let _ = first
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .await;
        sent_tx.send(()).unwrap();
        let (mut second, _) = listener.accept().await.unwrap();
        let (header, body) = read_request(&mut second).await;
        assert!(header.starts_with(&format!("GET /txs/{server_hash} HTTP/1.1")));
        assert!(body.is_empty());
        let body = serde_json::json!({"hash": server_hash, "slot": 42, "valid_contract": true})
            .to_string();
        second
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        // Recovery and retry must not open a replacement POST connection.
        assert!(
            tokio::time::timeout(Duration::from_millis(200), listener.accept())
                .await
                .is_err()
        );
    });
    let task_journal = Journal::new(
        state.db_executor.clone(),
        lease.clone(),
        DatabaseWorkload::Background,
    );
    let task_client = client.clone();
    let task_bytes = bytes.clone();
    let job = tokio::spawn(lease.run_until_closed(async move {
        submission::submit_once(&task_journal, &task_client, OPERATION, &task_bytes, CONTEXT).await
    }));
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(5), posted_rx)
            .await
            .unwrap()
            .unwrap(),
        bytes
    );
    lock(&state).await;
    assert!(
        job.await.unwrap().is_none(),
        "lock cancels the pass without aborting its task externally"
    );
    assert!(state.profile_operations.admit(&original_session).is_err());

    activate(&state, next_paths).await;
    assert_ne!(
        state.profile_operations.session().unwrap(),
        original_session
    );
    late_tx.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(5), sent_rx)
        .await
        .unwrap()
        .unwrap();
    assert!(
        submission::with_database(&state.db, |conn| submission::lookup(conn, OPERATION))
            .unwrap()
            .is_none()
    );
    lock(&state).await;

    activate(&state, original_paths).await;
    let saved = submission::with_database(&state.db, |conn| {
        let saved = submission::lookup(conn, OPERATION)?.unwrap();
        let stored: Vec<u8> = conn
            .query_row("SELECT signed_cbor FROM chain_submissions", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(stored, bytes);
        Ok(saved)
    })
    .unwrap();
    assert_eq!(saved.status, SubmissionStatus::OutcomeUnknown);
    assert_eq!(saved.tx_hash, expected);
    let lease = state
        .profile_operations
        .admit(&state.profile_operations.session().unwrap())
        .unwrap();
    let journal = Journal::new(
        state.db_executor.clone(),
        lease.clone(),
        DatabaseWorkload::Background,
    );
    let recovered = tokio::time::timeout(
        Duration::from_secs(5),
        lease.run_until_closed(submission::reconcile(&journal, &client, OPERATION)),
    )
    .await
    .unwrap()
    .unwrap()
    .unwrap()
    .unwrap();
    assert_eq!(recovered.status, SubmissionStatus::Confirmed);
    assert_eq!(recovered.confirmed_slot, Some(42));
    let retry = submission::submit_once(
        &journal,
        &client,
        OPERATION,
        &signed_transaction(300_000),
        CONTEXT,
    )
    .await
    .unwrap();
    assert_eq!(retry, recovered);
    // The journal's lease clone must not keep the final lock from draining.
    drop(journal);
    tokio::time::timeout(Duration::from_secs(5), server)
        .await
        .unwrap()
        .unwrap();
    lock(&state).await;
}
