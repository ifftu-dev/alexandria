//! A small in-process xAPI store for the host-seam tests. It answers
//! `about` and `statements` under `/xapi/` on a loopback port, records every
//! request, keeps the statements it accepted, and can be told to refuse, or
//! stopped and started again on the same port to play an offline store.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

#[derive(Debug, Clone)]
pub(super) struct StubRequest {
    pub method: String,
    pub path: String,
    /// Header names in lower case.
    pub headers: HashMap<String, String>,
    pub body: String,
}

#[derive(Default)]
struct State {
    requests: Vec<StubRequest>,
    accepted: Vec<Value>,
    versions: Vec<String>,
    /// Refuse every statements request with this status and body.
    refuse: Option<(u16, String)>,
    /// Refuse (400) any statements request whose body contains this text.
    refuse_containing: Option<String>,
}

pub(super) struct XapiStub {
    addr: SocketAddr,
    state: Arc<Mutex<State>>,
    task: Option<JoinHandle<()>>,
}

impl XapiStub {
    /// A store speaking `versions`.
    pub async fn start(versions: &[&str]) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind stub");
        let addr = listener.local_addr().expect("stub address");
        let state = Arc::new(Mutex::new(State {
            versions: versions.iter().map(|v| v.to_string()).collect(),
            ..State::default()
        }));
        let task = Some(tokio::spawn(serve(listener, state.clone())));
        Self { addr, state, task }
    }

    /// The xAPI base address a learner would enter.
    pub fn address(&self) -> String {
        format!("http://{}/xapi/", self.addr)
    }

    pub fn requests(&self) -> Vec<StubRequest> {
        self.state.lock().unwrap().requests.clone()
    }

    /// Requests that carried statements, accepted or not.
    pub fn statement_posts(&self) -> Vec<StubRequest> {
        self.requests()
            .into_iter()
            .filter(|r| r.method == "POST" && r.path == "/xapi/statements")
            .collect()
    }

    /// Every statement the store accepted, in arrival order.
    pub fn accepted(&self) -> Vec<Value> {
        self.state.lock().unwrap().accepted.clone()
    }

    pub fn refuse(&self, status: u16, body: &str) {
        self.state.lock().unwrap().refuse = Some((status, body.to_string()));
    }

    pub fn refuse_containing(&self, needle: &str) {
        self.state.lock().unwrap().refuse_containing = Some(needle.to_string());
    }

    pub fn accept_all(&self) {
        let mut state = self.state.lock().unwrap();
        state.refuse = None;
        state.refuse_containing = None;
    }

    /// Stops listening: connections are refused until [`Self::restart`].
    pub async fn stop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
            let _ = task.await;
        }
    }

    /// Listens again on the same port.
    pub async fn restart(&mut self) {
        let listener = TcpListener::bind(self.addr).await.expect("rebind stub");
        self.task = Some(tokio::spawn(serve(listener, self.state.clone())));
    }
}

impl Drop for XapiStub {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

async fn serve(listener: TcpListener, state: Arc<Mutex<State>>) {
    loop {
        let Ok((socket, _)) = listener.accept().await else {
            return;
        };
        tokio::spawn(handle(socket, state.clone()));
    }
}

async fn handle(mut socket: TcpStream, state: Arc<Mutex<State>>) {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 4096];
    let header_end = loop {
        if let Some(end) = buffer.windows(4).position(|w| w == b"\r\n\r\n") {
            break end;
        }
        match socket.read(&mut chunk).await {
            Ok(0) | Err(_) => return,
            Ok(n) => buffer.extend_from_slice(&chunk[..n]),
        }
    };
    let head = String::from_utf8_lossy(&buffer[..header_end]).to_string();
    let mut lines = head.split("\r\n");
    let mut request_line = lines.next().unwrap_or_default().split(' ');
    let method = request_line.next().unwrap_or_default().to_string();
    let path = request_line.next().unwrap_or_default().to_string();
    let headers: HashMap<String, String> = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
        .collect();
    let length: usize = headers
        .get("content-length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let mut body = buffer[header_end + 4..].to_vec();
    while body.len() < length {
        match socket.read(&mut chunk).await {
            Ok(0) | Err(_) => return,
            Ok(n) => body.extend_from_slice(&chunk[..n]),
        }
    }
    let request = StubRequest {
        method,
        path,
        headers,
        body: String::from_utf8_lossy(&body).to_string(),
    };
    let (status, response) = respond(&state, request);
    let reply = format!(
        "HTTP/1.1 {status} Stub\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{response}",
        response.len()
    );
    let _ = socket.write_all(reply.as_bytes()).await;
    let _ = socket.shutdown().await;
}

fn respond(state: &Mutex<State>, request: StubRequest) -> (u16, String) {
    let mut state = state.lock().unwrap();
    state.requests.push(request.clone());
    match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/xapi/about") => (
            200,
            serde_json::json!({ "version": state.versions }).to_string(),
        ),
        ("POST", "/xapi/statements") => {
            if let Some((status, body)) = state.refuse.clone() {
                return (status, body);
            }
            if let Some(needle) = &state.refuse_containing {
                if request.body.contains(needle.as_str()) {
                    return (400, "{\"error\":\"statement refused\"}".into());
                }
            }
            let statements: Vec<Value> = match serde_json::from_str(&request.body) {
                Ok(Value::Array(items)) => items,
                Ok(single) => vec![single],
                Err(_) => return (400, "{\"error\":\"not json\"}".into()),
            };
            let ids: Vec<Value> = statements.iter().map(|s| s["id"].clone()).collect();
            state.accepted.extend(statements);
            (200, Value::Array(ids).to_string())
        }
        _ => (404, "{}".into()),
    }
}
