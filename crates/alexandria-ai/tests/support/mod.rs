//! A scripted local HTTP server for adapter tests.
#![allow(dead_code)]

use std::time::Duration;

use alexandria_ai::StreamEvent;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::{mpsc, oneshot};

/// One scripted reply: a status line and headers, then body pieces written
/// with an optional pause before each.
pub struct Reply {
    pub head: String,
    pub body: Vec<(Duration, Vec<u8>)>,
}

impl Reply {
    pub fn sse(pieces: &[&[u8]]) -> Self {
        Self {
            head: "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n"
                .into(),
            body: pieces
                .iter()
                .map(|p| (Duration::ZERO, p.to_vec()))
                .collect(),
        }
    }

    pub fn json(status: &str, extra_headers: &str, body: &str) -> Self {
        Self {
            head: format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{extra_headers}Connection: close\r\n\r\n",
                body.len()
            ),
            body: vec![(Duration::ZERO, body.as_bytes().to_vec())],
        }
    }
}

/// Serves one connection and reports the raw request it received.
pub async fn serve(reply: Reply) -> (String, oneshot::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let (tx, rx) = oneshot::channel();
    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            let n = socket.read(&mut buf).await.unwrap();
            if n == 0 {
                break;
            }
            request.extend_from_slice(&buf[..n]);
            let text = String::from_utf8_lossy(&request);
            if let Some(end) = text.find("\r\n\r\n") {
                let length = text[..end]
                    .lines()
                    .find_map(|l| {
                        l.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .map(|v| v.trim().parse::<usize>().unwrap())
                    })
                    .unwrap_or(0);
                if request.len() >= end + 4 + length {
                    break;
                }
            }
        }
        let _ = tx.send(String::from_utf8_lossy(&request).into_owned());
        let _ = socket.write_all(reply.head.as_bytes()).await;
        for (pause, piece) in reply.body {
            tokio::time::sleep(pause).await;
            if socket.write_all(&piece).await.is_err() {
                return;
            }
            let _ = socket.flush().await;
        }
        let _ = socket.shutdown().await;
    });
    (base, rx)
}

pub async fn drain(mut rx: mpsc::Receiver<StreamEvent>) -> Vec<StreamEvent> {
    let mut events = Vec::new();
    while let Some(e) = rx.recv().await {
        events.push(e);
    }
    events
}
