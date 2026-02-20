use blake2::digest::consts::U32;
use blake2::{Blake2b, Digest};
use sha2::Sha256;

type Blake2b256 = Blake2b<U32>;

/// Compute Blake2b-256 hash of the input bytes.
pub fn blake2b_256(data: &[u8]) -> [u8; 32] {
    let mut hasher = Blake2b256::new();
    hasher.update(data);
    let result = hasher.finalize();
    let mut output = [0u8; 32];
    output.copy_from_slice(&result);
    output
}

/// Compute SHA-256 hash of the input bytes.
pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    let mut output = [0u8; 32];
    output.copy_from_slice(&result);
    output
}

/// Compute a deterministic entity ID from components.
/// Uses Blake2b-256 over the concatenation of all parts.
///
/// Example: `entity_id("stake1u8...", "course", "bafy...xyz")`
pub fn entity_id(parts: &[&str]) -> String {
    let combined = parts.join("|");
    let hash = blake2b_256(combined.as_bytes());
    hex::encode(hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blake2b_256_produces_32_bytes() {
        let hash = blake2b_256(b"hello");
        assert_eq!(hash.len(), 32);
    }

    #[test]
    fn blake2b_256_is_deterministic() {
        let a = blake2b_256(b"test input");
        let b = blake2b_256(b"test input");
        assert_eq!(a, b);
    }

    #[test]
    fn blake2b_256_differs_for_different_input() {
        let a = blake2b_256(b"input a");
        let b = blake2b_256(b"input b");
        assert_ne!(a, b);
    }

    #[test]
    fn sha256_produces_32_bytes() {
        let hash = sha256(b"hello");
        assert_eq!(hash.len(), 32);
    }

    #[test]
    fn entity_id_is_deterministic() {
        let a = entity_id(&["stake1u8abc", "course", "bafy123"]);
        let b = entity_id(&["stake1u8abc", "course", "bafy123"]);
        assert_eq!(a, b);
    }

    #[test]
    fn entity_id_differs_for_different_parts() {
        let a = entity_id(&["stake1u8abc", "course", "bafy123"]);
        let b = entity_id(&["stake1u8abc", "course", "bafy456"]);
        assert_ne!(a, b);
    }
}
