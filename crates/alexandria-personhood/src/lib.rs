pub mod groth16;
pub mod store;

use alexandria_verify::did::did_from_verifying_key;
use alexandria_verify::json::{decode_untrusted, JsonLimits};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use num_bigint::BigUint;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sha3::Keccak256;
use zeroize::Zeroizing;

pub use groth16::Groth16Proof;

pub const PURPOSE: &str = "personhood-receipt-v1";
pub const CIRCUIT: &str = "anon-aadhaar-v2.0.0-synthetic";
pub const TEST_ISSUER: &str =
    "15134874015316324267425466444584014077184337590635665158241104437045239495873";
pub const FIXTURE_TIMESTAMP: u64 = 1713555000;
const MAX_TIME: u64 = (1u64 << 53) - 1;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    #[error("invalid or oversized personhood encoding")]
    Encoding,
    #[error("invalid proof point")]
    Point,
    #[error("synthetic proof verification failed")]
    Proof,
    #[error("untrusted or inactive synthetic policy")]
    Policy,
    #[error("challenge account, session, or context mismatch")]
    Binding,
    #[error("invalid account or verifier signature")]
    Signature,
    #[error("challenge or document is outside its validity window")]
    Expired,
}

pub type Result<T> = std::result::Result<T, Error>;

pub fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    decode_untrusted(
        bytes,
        &JsonLimits {
            max_bytes: 16 * 1024,
            max_depth: 9,
            max_array_len: 16,
            max_object_entries: 32,
            max_string_bytes: 512,
        },
    )
    .map_err(|_| Error::Encoding)
}

pub fn canonical<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    serde_json_canonicalizer::to_vec(value).map_err(|_| Error::Encoding)
}

pub fn digest<T: Serialize>(value: &T) -> Result<String> {
    Ok(hex::encode(Sha256::digest(canonical(value)?)))
}

fn message<T: Serialize>(domain: &[u8], value: &T) -> Result<Vec<u8>> {
    let mut bytes = domain.to_vec();
    bytes.push(0);
    bytes.extend(canonical(value)?);
    Ok(bytes)
}

fn hex_bytes<const N: usize>(value: &str) -> Result<[u8; N]> {
    if value.len() != N * 2
        || !value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(Error::Encoding);
    }
    hex::decode(value)
        .map_err(|_| Error::Encoding)?
        .try_into()
        .map_err(|_| Error::Encoding)
}

fn key(value: &str) -> Result<VerifyingKey> {
    VerifyingKey::from_bytes(&hex_bytes(value)?).map_err(|_| Error::Encoding)
}

fn sign<T: Serialize>(domain: &[u8], value: &T, signer: &SigningKey) -> Result<String> {
    Ok(hex::encode(
        signer.sign(&message(domain, value)?).to_bytes(),
    ))
}

fn verify_signature<T: Serialize>(
    domain: &[u8],
    value: &T,
    signature: &str,
    signer: &VerifyingKey,
) -> Result<()> {
    signer
        .verify_strict(
            &message(domain, value)?,
            &Signature::from_bytes(&hex_bytes(signature)?),
        )
        .map_err(|_| Error::Signature)
}

pub fn local_verifier_key(account: &SigningKey) -> Result<SigningKey> {
    let source = Zeroizing::new(account.to_bytes());
    let mut seed = Zeroizing::new([0u8; 32]);
    hkdf::Hkdf::<Sha256>::new(
        Some(b"alexandria/local-synthetic-verifier/v1"),
        source.as_ref(),
    )
    .expand(b"signing-key", seed.as_mut())
    .map_err(|_| Error::Encoding)?;
    Ok(SigningKey::from_bytes(&seed))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReceiptKind {
    SyntheticDiagnostic,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub schema_version: u8,
    pub kind: ReceiptKind,
    pub network_id: String,
    pub circuit_id: String,
    pub verification_key_sha256: String,
    pub verifier_public_key: String,
    pub issuer_hash: String,
    pub issuer_revoked: bool,
    pub valid_from: u64,
    pub valid_until: u64,
    pub nullifier_seed: String,
    pub max_document_age_seconds: u64,
    pub max_future_skew_seconds: u64,
    pub challenge_ttl_seconds: u64,
    pub receipt_ttl_seconds: u64,
}

impl Policy {
    pub fn synthetic(network: &str, verifier: &VerifyingKey) -> Self {
        let seed = Sha256::digest(
            [
                b"alexandria/personhood-nullifier/synthetic/v1\0".as_slice(),
                network.as_bytes(),
            ]
            .concat(),
        );
        Self {
            schema_version: 1,
            kind: ReceiptKind::SyntheticDiagnostic,
            network_id: network.to_owned(),
            circuit_id: CIRCUIT.into(),
            verification_key_sha256: groth16::VERIFICATION_KEY_SHA256.into(),
            verifier_public_key: hex::encode(verifier.to_bytes()),
            issuer_hash: TEST_ISSUER.into(),
            issuer_revoked: false,
            valid_from: FIXTURE_TIMESTAMP,
            valid_until: 4102444800,
            nullifier_seed: (BigUint::from_bytes_be(&seed) >> 3usize).to_string(),
            // Only the fixed historical test document is accepted by this policy.
            max_document_age_seconds: 10 * 365 * 86400,
            max_future_skew_seconds: 300,
            challenge_ttl_seconds: 120,
            receipt_ttl_seconds: 86400,
        }
    }

    pub fn validate(&self, now: u64) -> Result<()> {
        if self.schema_version != 1
            || self.network_id.is_empty()
            || self.network_id.len() > 64
            || !self
                .network_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            || self.circuit_id != CIRCUIT
            || self.verification_key_sha256 != groth16::VERIFICATION_KEY_SHA256
            || self.issuer_hash != TEST_ISSUER
            || self.issuer_revoked
            || self.valid_from > now
            || now >= self.valid_until
            || self.valid_until > MAX_TIME
            || !(1..=120).contains(&self.challenge_ttl_seconds)
            || !(1..=86400).contains(&self.receipt_ttl_seconds)
            || self.max_document_age_seconds > 10 * 365 * 86400
            || self.max_future_skew_seconds > 300
        {
            return Err(Error::Policy);
        }
        key(&self.verifier_public_key)?;
        groth16::scalar(&self.nullifier_seed)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Challenge {
    pub schema_version: u8,
    pub kind: ReceiptKind,
    pub network_id: String,
    pub purpose: String,
    pub verifier_public_key: String,
    pub subject_did: String,
    pub subject_public_key: String,
    pub session_nonce: String,
    pub nonce: String,
    pub issued_at: u64,
    pub expires_at: u64,
    pub policy_digest: String,
    pub circuit_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SignedChallenge {
    pub body: Challenge,
    pub signature: String,
}

pub fn issue_challenge(
    policy: &Policy,
    account: &VerifyingKey,
    verifier: &SigningKey,
    session: [u8; 32],
    nonce: [u8; 32],
    now: u64,
) -> Result<SignedChallenge> {
    policy.validate(now)?;
    if hex::encode(verifier.verifying_key().to_bytes()) != policy.verifier_public_key {
        return Err(Error::Policy);
    }
    let body = Challenge {
        schema_version: 1,
        kind: ReceiptKind::SyntheticDiagnostic,
        network_id: policy.network_id.clone(),
        purpose: PURPOSE.into(),
        verifier_public_key: policy.verifier_public_key.clone(),
        subject_did: did_from_verifying_key(account).0,
        subject_public_key: hex::encode(account.to_bytes()),
        session_nonce: hex::encode(session),
        nonce: hex::encode(nonce),
        issued_at: now,
        expires_at: now
            .checked_add(policy.challenge_ttl_seconds)
            .ok_or(Error::Expired)?
            .min(policy.valid_until),
        policy_digest: digest(policy)?,
        circuit_id: policy.circuit_id.clone(),
    };
    let signature = sign(b"alexandria/personhood-challenge/v1", &body, verifier)?;
    Ok(SignedChallenge { body, signature })
}

pub fn signal_hash(challenge: &Challenge) -> Result<String> {
    let signal = Sha256::digest(message(b"alexandria/personhood-signal/v1", challenge)?);
    Ok((BigUint::from_bytes_be(&Keccak256::digest(signal)) >> 3usize).to_string())
}

pub fn validate_challenge(
    policy: &Policy,
    signed: &SignedChallenge,
    account: &VerifyingKey,
    now: u64,
) -> Result<()> {
    policy.validate(now)?;
    let c = &signed.body;
    hex_bytes::<32>(&c.nonce)?;
    hex_bytes::<32>(&c.session_nonce)?;
    if c.schema_version != 1
        || c.network_id != policy.network_id
        || c.purpose != PURPOSE
        || c.verifier_public_key != policy.verifier_public_key
        || c.policy_digest != digest(policy)?
        || c.circuit_id != policy.circuit_id
        || c.subject_public_key != hex::encode(account.to_bytes())
        || c.subject_did != did_from_verifying_key(account).0
    {
        return Err(Error::Binding);
    }
    if now < c.issued_at
        || now >= c.expires_at
        || c.expires_at > policy.valid_until
        || c.expires_at <= c.issued_at
        || c.expires_at - c.issued_at > policy.challenge_ttl_seconds
    {
        return Err(Error::Expired);
    }
    verify_signature(
        b"alexandria/personhood-challenge/v1",
        c,
        &signed.signature,
        &key(&policy.verifier_public_key)?,
    )
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Submission {
    pub challenge: SignedChallenge,
    pub proof: Groth16Proof,
    pub public_signals: [String; 9],
    pub account_signature: String,
}

impl Submission {
    pub fn signed(
        challenge: SignedChallenge,
        proof: Groth16Proof,
        public_signals: [String; 9],
        account: &SigningKey,
    ) -> Result<Self> {
        let mut value = Self {
            challenge,
            proof,
            public_signals,
            account_signature: String::new(),
        };
        value.account_signature = sign(
            b"alexandria/personhood-submission/v1",
            &value.payload(),
            account,
        )?;
        Ok(value)
    }

    fn payload(&self) -> (&SignedChallenge, &Groth16Proof, &[String; 9]) {
        (&self.challenge, &self.proof, &self.public_signals)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ReceiptBody {
    pub id: String,
    pub kind: ReceiptKind,
    pub network_id: String,
    pub subject_did: String,
    pub verifier_public_key: String,
    pub policy_digest: String,
    pub circuit_id: String,
    pub submission_digest: String,
    pub created_at: u64,
    pub expires_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub body: ReceiptBody,
    pub signature: String,
}

pub struct VerifiedSubmission {
    submission: Submission,
    document_expiry: u64,
}

fn verify_context(
    policy: &Policy,
    stored: &SignedChallenge,
    submission: &Submission,
    account: &VerifyingKey,
    now: u64,
) -> Result<u64> {
    if &submission.challenge != stored {
        return Err(Error::Binding);
    }
    validate_challenge(policy, stored, account, now)?;
    verify_signature(
        b"alexandria/personhood-submission/v1",
        &submission.payload(),
        &submission.account_signature,
        account,
    )?;
    let signals = &submission.public_signals;
    for signal in signals {
        groth16::scalar(signal)?;
    }
    if signals[0] != policy.issuer_hash
        || signals[3..7].iter().any(|s| s != "0")
        || signals[7] != policy.nullifier_seed
        || signals[8] != signal_hash(&stored.body)?
    {
        return Err(Error::Binding);
    }
    let timestamp: u64 = signals[2].parse().map_err(|_| Error::Encoding)?;
    // This verifier cannot be repurposed for real documents by changing a policy window.
    if timestamp != FIXTURE_TIMESTAMP {
        return Err(Error::Policy);
    }
    let document_expiry = timestamp
        .checked_add(policy.max_document_age_seconds)
        .ok_or(Error::Expired)?;
    if timestamp > now.saturating_add(policy.max_future_skew_seconds) || now >= document_expiry {
        return Err(Error::Expired);
    }
    Ok(document_expiry)
}

pub fn verify_submission(
    policy: &Policy,
    stored: &SignedChallenge,
    submission: Submission,
    account: &VerifyingKey,
    now: u64,
) -> Result<VerifiedSubmission> {
    let document_expiry = verify_context(policy, stored, &submission, account, now)?;
    groth16::verify(&submission.proof, &submission.public_signals)?;
    Ok(VerifiedSubmission {
        submission,
        document_expiry,
    })
}

impl VerifiedSubmission {
    pub fn challenge(&self) -> &SignedChallenge {
        &self.submission.challenge
    }
    pub fn submission_digest(&self) -> Result<String> {
        digest(&self.submission)
    }

    pub fn receipt(
        &self,
        policy: &Policy,
        account: &VerifyingKey,
        verifier: &SigningKey,
        now: u64,
    ) -> Result<Receipt> {
        verify_context(policy, self.challenge(), &self.submission, account, now)?;
        if hex::encode(verifier.verifying_key().to_bytes()) != policy.verifier_public_key {
            return Err(Error::Policy);
        }
        let body = ReceiptBody {
            id: self.challenge().body.nonce.clone(),
            kind: ReceiptKind::SyntheticDiagnostic,
            network_id: policy.network_id.clone(),
            subject_did: self.challenge().body.subject_did.clone(),
            verifier_public_key: policy.verifier_public_key.clone(),
            policy_digest: digest(policy)?,
            circuit_id: policy.circuit_id.clone(),
            submission_digest: self.submission_digest()?,
            created_at: now,
            expires_at: now
                .checked_add(policy.receipt_ttl_seconds)
                .ok_or(Error::Expired)?
                .min(self.document_expiry)
                .min(policy.valid_until),
        };
        let signature = sign(
            b"alexandria/personhood-diagnostic-receipt/v1",
            &body,
            verifier,
        )?;
        Ok(Receipt { body, signature })
    }
}

pub fn verify_receipt(receipt: &Receipt, verifier: &VerifyingKey) -> Result<()> {
    if receipt.body.verifier_public_key != hex::encode(verifier.to_bytes()) {
        return Err(Error::Binding);
    }
    verify_signature(
        b"alexandria/personhood-diagnostic-receipt/v1",
        &receipt.body,
        &receipt.signature,
        verifier,
    )
}
