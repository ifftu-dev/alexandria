//! Per-device secret used to scope the libp2p PeerId to this install.
//!
//! Without this, two devices unlocked with the same vault would derive the
//! same Ed25519 key from the Cardano payment key, collide on a single PeerId
//! at the libp2p layer, and appear as one node to peers/observers. Mixing
//! a per-device secret into the key derivation gives each install a distinct
//! PeerId while keeping the on-chain DID/wallet identity shared.
//!
//! The device secret is 32 random bytes generated once and persisted at
//! `app_data_dir/device_id.bin` with 0600 permissions on Unix. It never
//! leaves the device.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use rand::rngs::OsRng;
use rand::RngCore;

const FILE_NAME: &str = "device_id.bin";
const DEVICE_ID_LEN: usize = 32;

fn device_id_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(FILE_NAME)
}

/// Load the per-device secret from `app_data_dir`, or generate and persist
/// a fresh 32-byte value on first use.
pub fn load_or_create(app_data_dir: &Path) -> io::Result<[u8; DEVICE_ID_LEN]> {
    fs::create_dir_all(app_data_dir)?;
    let path = device_id_path(app_data_dir);

    match fs::read(&path) {
        Ok(bytes) if bytes.len() == DEVICE_ID_LEN => {
            let mut out = [0u8; DEVICE_ID_LEN];
            out.copy_from_slice(&bytes);
            Ok(out)
        }
        Ok(_) | Err(_) => {
            let mut bytes = [0u8; DEVICE_ID_LEN];
            OsRng.fill_bytes(&mut bytes);
            write_atomic(&path, &bytes)?;
            Ok(bytes)
        }
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let tmp = path.with_extension("bin.tmp");
    fs::write(&tmp, bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&tmp)?.permissions();
        perms.set_mode(0o600);
        fs::set_permissions(&tmp, perms)?;
    }
    fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn load_or_create_persists_across_calls() {
        let dir = TempDir::new().unwrap();
        let first = load_or_create(dir.path()).unwrap();
        let second = load_or_create(dir.path()).unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn distinct_dirs_yield_distinct_ids() {
        let a = TempDir::new().unwrap();
        let b = TempDir::new().unwrap();
        assert_ne!(
            load_or_create(a.path()).unwrap(),
            load_or_create(b.path()).unwrap()
        );
    }

    #[test]
    fn corrupt_file_is_regenerated() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join(FILE_NAME), b"too-short").unwrap();
        let id = load_or_create(dir.path()).unwrap();
        assert_eq!(id.len(), DEVICE_ID_LEN);
    }
}
