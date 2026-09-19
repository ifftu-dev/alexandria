#[cfg(unix)]
use serde::Deserialize;
use serde_json::Value;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct Broker {
    connection_file: PathBuf,
}

#[cfg(unix)]
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Credential {
    socket: PathBuf,
    token: String,
}

impl Broker {
    pub fn from_env() -> Option<Self> {
        std::env::var_os("ALEXANDRIA_MCP_CONNECTION_FILE").map(|path| Self {
            connection_file: path.into(),
        })
    }

    /// Whether the app is currently offering this connection: the file it
    /// wrote is still there and still private to this user. Lock, profile
    /// switch, revocation and expiry all remove it, and after any of those the
    /// tools behind it cannot answer.
    pub fn available(&self) -> bool {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let Ok(metadata) = std::fs::symlink_metadata(&self.connection_file) else {
                return false;
            };
            metadata.is_file()
                && !metadata.file_type().is_symlink()
                && metadata.permissions().mode() & 0o077 == 0
                && metadata.len() <= 4096
        }
        #[cfg(not(unix))]
        {
            false
        }
    }

    pub async fn call(&self, mut request: Value) -> Result<Value, String> {
        #[cfg(unix)]
        {
            use std::io::Read;
            use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
            use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};
            // Credentials are read on every request, never echoed or cached in tool output.
            let metadata = std::fs::symlink_metadata(&self.connection_file)
                .map_err(|_| "unavailable: connection file is unavailable".to_string())?;
            if !metadata.is_file()
                || metadata.file_type().is_symlink()
                || metadata.permissions().mode() & 0o077 != 0
                || metadata.len() > 4096
            {
                return Err(
                    "permission_denied: connection file must be a private regular file".into(),
                );
            }
            let mut bytes = Vec::new();
            std::fs::OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW)
                .open(&self.connection_file)
                .map_err(|_| "unavailable: connection file is unavailable".to_string())?
                .take(4097)
                .read_to_end(&mut bytes)
                .map_err(|_| "unavailable: could not read connection file".to_string())?;
            if bytes.len() > 4096 {
                return Err("invalid_input: connection file too large".into());
            }
            let credential: Credential = serde_json::from_slice(&bytes)
                .map_err(|_| "invalid_input: invalid connection file".to_string())?;
            request["token"] = Value::String(credential.token);
            let mut frame =
                serde_json::to_vec(&request).map_err(|_| "invalid_input".to_string())?;
            frame.push(b'\n');
            if frame.len() > 262144 {
                return Err("invalid_input: request too large".into());
            }
            let exchange = async {
                let stream = tokio::net::UnixStream::connect(credential.socket)
                    .await
                    .map_err(|_| {
                        "unavailable: open Alexandria and grant assistant access again".to_string()
                    })?;
                let (reader, mut writer) = stream.into_split();
                writer
                    .write_all(&frame)
                    .await
                    .map_err(|_| "unavailable: broker disconnected".to_string())?;
                let mut reader = tokio::io::BufReader::new(reader.take(1048577));
                let mut bytes = Vec::new();
                reader
                    .read_until(b'\n', &mut bytes)
                    .await
                    .map_err(|_| "unavailable: broker disconnected".to_string())?;
                if bytes.len() > 1048576 || bytes.last() != Some(&b'\n') {
                    return Err("unavailable: invalid broker response".into());
                }
                let mut response: Value = serde_json::from_slice(&bytes)
                    .map_err(|_| "unavailable: invalid broker response".to_string())?;
                if let Some(error) = response.get("error").and_then(Value::as_str) {
                    return Err(error.to_string());
                }
                response
                    .get_mut("result")
                    .map(Value::take)
                    .ok_or_else(|| "unavailable: invalid broker response".to_string())
            };
            // Longer than the broker's own limit, which covers fetching a lesson from peers.
            tokio::time::timeout(std::time::Duration::from_secs(20), exchange)
                .await
                .map_err(|_| "unavailable: broker request timed out".to_string())?
        }
        #[cfg(not(unix))]
        {
            let _ = (&self.connection_file, &mut request);
            Err("unavailable: local assistant access currently requires macOS or Linux".into())
        }
    }
}
