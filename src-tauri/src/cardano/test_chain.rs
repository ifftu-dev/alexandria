//! Test-only stand-ins for a chain provider and an unlocked profile.
//!
//! [`FakeChain`] is a loopback HTTP server speaking the small Blockfrost
//! subset the builders and recovery paths use; it never contacts a real
//! network. [`TestProfile`] puts a migrated database behind a real
//! [`DatabaseExecutor`] and profile admission, so journal phases run exactly
//! as they do in production.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rusqlite::Connection;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::task::JoinSet;

use super::blockfrost::{BlockfrostClient, RequestLimits};
use super::submission::Journal;
use crate::db::executor::{DatabaseExecutor, DatabaseWorkload};
use crate::db::Database;
use crate::profile::scope::Admission;

/// Short deadlines so a stalled provider times out quickly in tests.
pub(crate) const SHORT_LIMITS: RequestLimits = RequestLimits {
    connect: Duration::from_secs(2),
    total: Duration::from_millis(300),
};

pub(crate) enum Reply {
    Json(u16, String),
    /// Read the whole request, then never answer it.
    Stall,
}

type Route = dyn Fn(&str, &str, &[u8]) -> Reply + Send + Sync;

pub(crate) struct FakeChain {
    base_url: String,
    requests: Arc<Mutex<Vec<String>>>,
    server: tokio::task::JoinHandle<()>,
}

impl FakeChain {
    /// Serve every request with `route(method, path, body)`.
    pub(crate) async fn start(
        route: impl Fn(&str, &str, &[u8]) -> Reply + Send + Sync + 'static,
    ) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let route: Arc<Route> = Arc::new(route);
        let recorded = requests.clone();
        let server = tokio::spawn(async move {
            // Owning the connections here aborts stalled ones with the server.
            let mut connections = JoinSet::new();
            while let Ok((socket, _)) = listener.accept().await {
                connections.spawn(serve(socket, route.clone(), recorded.clone()));
            }
        });
        Self {
            base_url,
            requests,
            server,
        }
    }

    /// A provider for the metadata-only builders whose submit endpoint
    /// receives the signed bytes but never answers. Receipts are "not found"
    /// until the returned flag is set, then report inclusion at `slot`.
    pub(crate) async fn stalled_submit(slot: u64) -> (Self, Arc<AtomicBool>) {
        let included = Arc::new(AtomicBool::new(false));
        let visible = included.clone();
        let chain = Self::start(move |method, path, _| {
            let hash = path.strip_prefix("/txs/").filter(|hash| hash.len() == 64);
            match (method, path, hash) {
                ("GET", _, _) if path.starts_with("/addresses/") && path.ends_with("/utxos") => {
                    json(serde_json::json!([{
                        "tx_hash": "11".repeat(32),
                        "tx_index": 0,
                        "amount": [{"unit": "lovelace", "quantity": "10000000"}],
                    }]))
                }
                ("GET", "/epochs/latest/parameters", _) => json(
                    serde_json::json!({"min_fee_a": 44, "min_fee_b": 155_381, "max_tx_size": 16_384}),
                ),
                ("GET", "/blocks/latest", _) => json(serde_json::json!({"slot": 1_000})),
                ("POST", "/tx/submit", _) => Reply::Stall,
                ("GET", _, Some(hash)) if visible.load(Ordering::Acquire) => json(
                    serde_json::json!({"hash": hash, "slot": slot, "valid_contract": true}),
                ),
                _ => Reply::Json(404, r#"{"status_code":404}"#.into()),
            }
        })
        .await;
        (chain, included)
    }

    pub(crate) fn client(&self, limits: RequestLimits) -> BlockfrostClient {
        BlockfrostClient::with_limits("test".into(), self.base_url.clone(), limits).unwrap()
    }

    /// `"METHOD /path"` for every request received so far, in arrival order.
    pub(crate) fn requests(&self) -> Vec<String> {
        self.requests.lock().unwrap().clone()
    }

    pub(crate) fn count(&self, request: &str) -> usize {
        self.requests()
            .iter()
            .filter(|seen| seen.as_str() == request)
            .count()
    }
}

impl Drop for FakeChain {
    fn drop(&mut self) {
        self.server.abort();
    }
}

fn json(value: serde_json::Value) -> Reply {
    Reply::Json(200, value.to_string())
}

async fn serve(mut socket: TcpStream, route: Arc<Route>, requests: Arc<Mutex<Vec<String>>>) {
    let Some((method, path, body)) = read_request(&mut socket).await else {
        return;
    };
    requests.lock().unwrap().push(format!("{method} {path}"));
    match route(&method, &path, &body) {
        Reply::Json(status, body) => {
            let response = format!(
                "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = socket.write_all(response.as_bytes()).await;
            let _ = socket.shutdown().await;
        }
        Reply::Stall => std::future::pending::<()>().await,
    }
}

async fn read_request(socket: &mut TcpStream) -> Option<(String, String, Vec<u8>)> {
    let mut bytes = Vec::new();
    loop {
        let mut chunk = [0; 4096];
        let count = socket.read(&mut chunk).await.ok()?;
        if count == 0 {
            return None;
        }
        bytes.extend_from_slice(&chunk[..count]);
        assert!(bytes.len() < 256 * 1024, "unexpected request size");
        let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") else {
            continue;
        };
        let header = String::from_utf8(bytes[..end].to_vec()).ok()?;
        let length = header
            .lines()
            .filter_map(|line| line.split_once(':'))
            .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
            .map(|(_, value)| value.trim().parse::<usize>().unwrap())
            .unwrap_or(0);
        if bytes.len() < end + 4 + length {
            continue;
        }
        let mut request_line = header.lines().next()?.split_whitespace();
        let method = request_line.next()?.to_owned();
        let path = request_line.next()?.to_owned();
        return Some((method, path, bytes[end + 4..end + 4 + length].to_vec()));
    }
}

/// A migrated database behind a real executor and an open profile session.
pub(crate) struct TestProfile {
    pub(crate) db: Arc<Mutex<Option<Database>>>,
    pub(crate) executor: DatabaseExecutor,
    pub(crate) admission: Admission,
    session: String,
}

impl TestProfile {
    pub(crate) fn new(database: Database) -> Self {
        let db = Arc::new(Mutex::new(Some(database)));
        let admission = Admission::default();
        let session = admission.close();
        assert!(admission.open(&session));
        Self {
            executor: DatabaseExecutor::new(db.clone()),
            db,
            admission,
            session,
        }
    }

    pub(crate) fn migrated() -> Self {
        let database = Database::open_in_memory().unwrap();
        database.run_migrations().unwrap();
        Self::new(database)
    }

    /// A journal holding a fresh lease on the current session.
    pub(crate) fn journal(&self, workload: DatabaseWorkload) -> Journal {
        Journal::new(
            self.executor.clone(),
            self.admission.admit(&self.session).unwrap(),
            workload,
        )
    }

    pub(crate) fn background(&self) -> Journal {
        self.journal(DatabaseWorkload::Background)
    }

    /// Direct access for arranging and asserting state outside executor jobs.
    pub(crate) fn with_conn<T>(&self, work: impl FnOnce(&Connection) -> T) -> T {
        let guard = self.db.lock().unwrap();
        work(guard.as_ref().unwrap().conn())
    }
}

pub(crate) fn wallet() -> crate::crypto::wallet::Wallet {
    crate::crypto::wallet::wallet_from_mnemonic(
        "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about",
    )
    .unwrap()
}
