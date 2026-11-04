//! One durable, recipient-bound destination. Reading does not consume it;
//! only the matching admitted profile may acknowledge successful navigation.
use super::recipients;
use super::NotificationError;
use super::NotificationOpen;
use crate::crypto::{group_key, keystore::Keystore};
use crate::profile::{ProfileId, ProfilePaths};
use crate::{profile::scope::ProfileLease, AppState};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Mutex;
use x25519_dalek::{PublicKey, StaticSecret};
use zeroize::Zeroizing;

const PUBLIC_KEY: &str = "notification_open.pub";

fn secret(keystore: &Keystore, profile: &str) -> Result<StaticSecret, NotificationError> {
    let db_key = Zeroizing::new(keystore.derive_db_key());
    let mut bytes = Zeroizing::new([0; 32]);
    hkdf::Hkdf::<Sha256>::new(Some(profile.as_bytes()), db_key.as_ref())
        .expand(b"alexandria-notification-open-v1", bytes.as_mut())
        .map_err(|_| NotificationError::KeyDerivation)?;
    Ok(StaticSecret::from(*bytes))
}

/// Publish only an encryption key during the existing profile startup. The
/// private key remains derivable only from this profile's unlocked vault.
pub(crate) fn prepare(paths: &ProfilePaths, keystore: &Keystore) -> Result<(), NotificationError> {
    let key = PublicKey::from(&secret(keystore, paths.id.as_str())?);
    let temp = paths.root.join(format!("{PUBLIC_KEY}.tmp"));
    std::fs::write(&temp, key.as_bytes())?;
    Ok(std::fs::rename(temp, paths.root.join(PUBLIC_KEY))?)
}

#[derive(Serialize, Deserialize)]
struct SealedOpen {
    routing: NotificationOpen,
    ephemeral: [u8; 32],
    encrypted_key: Vec<u8>,
    ciphertext: Vec<u8>,
}

static FILE_LOCK: Mutex<()> = Mutex::new(());
const FILE: &str = "notification_open.json";
/// Version 2 is snake_case throughout. Earlier files (camelCase, or the
/// unversioned plaintext prototype) are pre-launch data, discarded unread.
const FORMAT_VERSION: u8 = 2;

#[derive(Serialize, Deserialize)]
struct OpenState {
    format_version: u8,
    pending: Option<SealedOpen>,
    consumed: Vec<String>,
}

impl Default for OpenState {
    fn default() -> Self {
        Self {
            format_version: FORMAT_VERSION,
            pending: None,
            consumed: Vec::new(),
        }
    }
}

fn load(state: &AppState) -> Result<OpenState, NotificationError> {
    match std::fs::read(state.app_data_dir.join(FILE)) {
        Ok(bytes) => {
            let value: serde_json::Value = serde_json::from_slice(&bytes)?;
            if value.get("format_version").is_none() {
                // Pre-launch data is incompatible. Do not preserve its
                // activity history or guess a locked vault key.
                std::fs::remove_file(state.app_data_dir.join(FILE))?;
                let _ = std::fs::remove_file(state.app_data_dir.join(format!("{FILE}.tmp")));
                return Ok(OpenState::default());
            }
            let opens: OpenState = serde_json::from_value(value)?;
            if opens.format_version != FORMAT_VERSION {
                return Err(NotificationError::UnsupportedOpenFormat);
            }
            Ok(opens)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(OpenState::default()),
        Err(e) => Err(e.into()),
    }
}

fn save(state: &AppState, opens: &OpenState) -> Result<(), NotificationError> {
    use std::io::Write;
    let path = state.app_data_dir.join(FILE);
    let tmp = state.app_data_dir.join(format!("{FILE}.tmp"));
    let mut file = std::fs::File::create(&tmp)?;
    file.write_all(&serde_json::to_vec(opens)?)?;
    file.sync_all()?;
    Ok(std::fs::rename(tmp, path)?)
}

fn valid_routing(state: &AppState, intent: &NotificationOpen) -> bool {
    recipients::valid_profile(state, &intent.profile_id, &intent.network_id)
        && intent.recipient.as_ref().is_none_or(|recipient| {
            recipients::resolve(state, recipient).is_some_and(|binding| {
                binding.profile_id == intent.profile_id && binding.network_id == intent.network_id
            })
        })
}

fn valid(state: &AppState, intent: &NotificationOpen) -> bool {
    valid_routing(state, intent)
        && intent.route.starts_with('/')
        && !intent.route.starts_with("//")
        && !intent.route.contains('\\')
        && !intent.route.chars().any(char::is_control)
}

pub fn hold(
    state: &AppState,
    mut intent: NotificationOpen,
) -> Result<Option<NotificationOpen>, NotificationError> {
    if !valid(state, &intent) {
        return Ok(None);
    }
    // The event contains random notification/recipient UUIDs. Retain only an
    // opaque digest for duplicate suppression, never event kinds or reference IDs.
    intent.id = hex::encode(Sha256::digest(intent.id.as_bytes()));
    let _guard = FILE_LOCK.lock()?;
    let mut opens = load(state)?;
    if opens.consumed.contains(&intent.id) {
        return Ok(None);
    }
    if opens
        .pending
        .as_ref()
        .is_some_and(|p| p.routing.id == intent.id)
    {
        return Ok(None);
    }
    let paths = state.profile_manager.paths_for(
        &ProfileId::parse(&intent.profile_id)
            .map_err(|e| NotificationError::Profile(e.to_string()))?,
    );
    let bytes: [u8; 32] = std::fs::read(paths.root.join(PUBLIC_KEY))?
        .try_into()
        .map_err(|_| NotificationError::InvalidKey)?;
    let ephemeral = StaticSecret::random_from_rng(rand::rngs::OsRng);
    let key = Zeroizing::new(group_key::generate_group_key());
    let encrypted_key =
        group_key::encrypt_key_for_member(&key, &PublicKey::from(bytes), &ephemeral)
            .map_err(|e| NotificationError::Crypto(e.to_string()))?;
    let plaintext = Zeroizing::new(serde_json::to_vec(&intent)?);
    let ciphertext = group_key::encrypt_message(&key, &plaintext)
        .map_err(|e| NotificationError::Crypto(e.to_string()))?;
    let mut routing = intent.clone();
    routing.route.clear();
    opens.pending = Some(SealedOpen {
        routing,
        ephemeral: PublicKey::from(&ephemeral).to_bytes(),
        encrypted_key,
        ciphertext,
    });
    save(state, &opens)?;
    Ok(Some(intent))
}

pub async fn pending(state: &AppState) -> Result<Option<NotificationOpen>, NotificationError> {
    let sealed = {
        let _guard = FILE_LOCK.lock()?;
        load(state)?.pending
    };
    let Some(sealed) = sealed else {
        return Ok(None);
    };
    if !valid_routing(state, &sealed.routing) {
        return Ok(None);
    }
    let Some(session) = state.profile_operations.session() else {
        return Ok(Some(sealed.routing));
    };
    let lease = state
        .profile_operations
        .admit(&session)
        .map_err(NotificationError::Profile)?;
    if state.active_id().as_ref().map(|p| p.as_str()) != Some(sealed.routing.profile_id.as_str()) {
        return Ok(Some(sealed.routing));
    }
    let keystore = state.keystore.lock().await;
    let private = secret(
        keystore.as_ref().ok_or(NotificationError::VaultLocked)?,
        &sealed.routing.profile_id,
    )?;
    let key = Zeroizing::new(
        group_key::decrypt_key_from_member(
            &sealed.encrypted_key,
            &PublicKey::from(sealed.ephemeral),
            &private,
        )
        .map_err(|e| NotificationError::Crypto(e.to_string()))?,
    );
    let plaintext = Zeroizing::new(
        group_key::decrypt_message(&key, &sealed.ciphertext)
            .map_err(|e| NotificationError::Crypto(e.to_string()))?,
    );
    let intent: NotificationOpen = serde_json::from_slice(&plaintext)?;
    let mut routing = intent.clone();
    routing.route.clear();
    if routing != sealed.routing || !valid(state, &intent) {
        return Err(NotificationError::InvalidEnvelope);
    }
    lease
        .with_current(|| Ok(Some(intent)))
        .map_err(NotificationError::Profile)
}

pub fn consume(state: &AppState, lease: &ProfileLease, id: &str) -> Result<(), NotificationError> {
    lease
        .with_current(|| Ok(consume_current(state, id)))
        .map_err(NotificationError::Profile)?
}

/// [`consume`] while its lease is held current.
fn consume_current(state: &AppState, id: &str) -> Result<(), NotificationError> {
    let _guard = FILE_LOCK.lock()?;
    let mut opens = load(state)?;
    let Some(sealed) = &opens.pending else {
        return Ok(());
    };
    let intent = &sealed.routing;
    if intent.id != id {
        return Ok(());
    }
    if !valid_routing(state, intent)
        || state.active_id().as_ref().map(|p| p.as_str()) != Some(intent.profile_id.as_str())
    {
        return Err(NotificationError::OtherProfile);
    }
    opens.pending = None;
    opens.consumed.push(id.into());
    if opens.consumed.len() > 256 {
        opens.consumed.remove(0);
    }
    save(state, &opens)
}

/// Local banner extras are produced by this app; remote data cannot use this adapter.
pub fn local_tap(
    state: &AppState,
    extra: &serde_json::Map<String, serde_json::Value>,
) -> Result<Option<NotificationOpen>, NotificationError> {
    let text = |key: &str| extra.get(key).and_then(serde_json::Value::as_str);
    let (Some(profile), Some(network), Some(notification), Some(route)) = (
        text("profileId"),
        text("networkId"),
        text("notificationId"),
        text("route"),
    ) else {
        return Ok(None);
    };
    if !super::push::canonical_uuid(notification) {
        return Ok(None);
    }
    hold(
        state,
        NotificationOpen {
            id: format!("local:{profile}:{notification}"),
            profile_id: profile.into(),
            recipient: None,
            route: route.into(),
            network_id: network.into(),
        },
    )
}
