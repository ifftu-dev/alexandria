use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum SigningError {
    #[error("invalid key bytes: {0}")]
    InvalidKey(String),
    #[error("signature verification failed")]
    VerificationFailed,
    #[error("invalid signature bytes")]
    InvalidSignature,
}

/// A signed message with its Ed25519 signature and the signer's public key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedMessage {
    /// The raw message bytes (typically JSON-serialized payload).
    pub payload: Vec<u8>,
    /// Ed25519 signature over the payload.
    pub signature: Vec<u8>,
    /// The signer's Ed25519 public key (32 bytes).
    pub public_key: Vec<u8>,
}

/// Sign a message with the given Ed25519 signing key.
pub fn sign(message: &[u8], key: &SigningKey) -> SignedMessage {
    let signature = key.sign(message);
    SignedMessage {
        payload: message.to_vec(),
        signature: signature.to_bytes().to_vec(),
        public_key: key.verifying_key().to_bytes().to_vec(),
    }
}

/// Verify a signed message.
pub fn verify(signed: &SignedMessage) -> Result<(), SigningError> {
    let public_key_bytes: [u8; 32] = signed
        .public_key
        .as_slice()
        .try_into()
        .map_err(|_| SigningError::InvalidKey("public key must be 32 bytes".into()))?;

    let verifying_key = VerifyingKey::from_bytes(&public_key_bytes)
        .map_err(|e| SigningError::InvalidKey(e.to_string()))?;

    let sig_bytes: [u8; 64] = signed
        .signature
        .as_slice()
        .try_into()
        .map_err(|_| SigningError::InvalidSignature)?;

    let signature = Signature::from_bytes(&sig_bytes);

    verifying_key
        .verify(&signed.payload, &signature)
        .map_err(|_| SigningError::VerificationFailed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_and_verify_roundtrip() {
        let mut rng = rand::thread_rng();
        let key = SigningKey::generate(&mut rng);
        let message = b"hello alexandria";

        let signed = sign(message, &key);
        assert!(verify(&signed).is_ok());
    }

    #[test]
    fn verify_rejects_tampered_payload() {
        let mut rng = rand::thread_rng();
        let key = SigningKey::generate(&mut rng);
        let message = b"original message";

        let mut signed = sign(message, &key);
        signed.payload = b"tampered message".to_vec();

        assert!(verify(&signed).is_err());
    }

    #[test]
    fn verify_rejects_wrong_key() {
        let mut rng = rand::thread_rng();
        let key1 = SigningKey::generate(&mut rng);
        let key2 = SigningKey::generate(&mut rng);
        let message = b"test message";

        let mut signed = sign(message, &key1);
        // Replace public key with a different one
        signed.public_key = key2.verifying_key().to_bytes().to_vec();

        assert!(verify(&signed).is_err());
    }
}
