//! One durable, recipient-bound destination. Reading does not consume it;
//! only the matching admitted profile may acknowledge successful navigation.
use super::recipients;
use crate::crypto::{group_key, keystore::Keystore};
use crate::profile::{ProfileId, ProfilePaths};
use crate::{profile::scope::ProfileLease, AppState};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Mutex;
use x25519_dalek::{PublicKey, StaticSecret};
use zeroize::Zeroizing;

const PUBLIC_KEY: &str = "notification_open.pub";

fn secret(keystore: &Keystore, profile: &str) -> Result<StaticSecret, String> {
    let db_key = Zeroizing::new(keystore.derive_db_key());
    let mut bytes = Zeroizing::new([0; 32]);
    hkdf::Hkdf::<Sha256>::new(Some(profile.as_bytes()), db_key.as_ref())
        .expand(b"alexandria-notification-open-v1", bytes.as_mut())
        .map_err(|_| "notification key derivation failed")?;
    Ok(StaticSecret::from(*bytes))
}

/// Publish only an encryption key during the existing profile startup. The
/// private key remains derivable only from this profile's unlocked vault.
pub(crate) fn prepare(paths: &ProfilePaths, keystore: &Keystore) -> Result<(), String> {
    let key = PublicKey::from(&secret(keystore, paths.id.as_str())?);
    let temp = paths.root.join(format!("{PUBLIC_KEY}.tmp"));
    std::fs::write(&temp, key.as_bytes()).map_err(|e| e.to_string())?;
    std::fs::rename(temp, paths.root.join(PUBLIC_KEY)).map_err(|e| e.to_string())
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

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NotificationOpen {
    pub id: String,
    pub profile_id: String,
    pub recipient: Option<String>,
    pub route: String,
    pub network_id: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OpenState {
    format_version: u8,
    pending: Option<SealedOpen>,
    consumed: Vec<String>,
}

impl Default for OpenState {
    fn default() -> Self {
        Self {
            format_version: 1,
            pending: None,
            consumed: Vec::new(),
        }
    }
}

fn load(state: &AppState) -> Result<OpenState, String> {
    match std::fs::read(state.app_data_dir.join(FILE)) {
        Ok(bytes) => {
            let value: serde_json::Value =
                serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            if value.get("formatVersion").is_none() {
                // Pre-launch plaintext prototype data is incompatible. Do not
                // preserve its activity history or guess a locked vault key.
                std::fs::remove_file(state.app_data_dir.join(FILE)).map_err(|e| e.to_string())?;
                let _ = std::fs::remove_file(state.app_data_dir.join(format!("{FILE}.tmp")));
                return Ok(OpenState::default());
            }
            let opens: OpenState = serde_json::from_value(value).map_err(|e| e.to_string())?;
            if opens.format_version != 1 {
                return Err("unsupported notification open format".into());
            }
            Ok(opens)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(OpenState::default()),
        Err(e) => Err(e.to_string()),
    }
}

fn save(state: &AppState, opens: &OpenState) -> Result<(), String> {
    use std::io::Write;
    let path = state.app_data_dir.join(FILE);
    let tmp = state.app_data_dir.join(format!("{FILE}.tmp"));
    let mut file = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
    file.write_all(&serde_json::to_vec(opens).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    std::fs::rename(tmp, path).map_err(|e| e.to_string())
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
) -> Result<Option<NotificationOpen>, String> {
    if !valid(state, &intent) {
        return Ok(None);
    }
    // The event contains random notification/recipient UUIDs. Retain only an
    // opaque digest for duplicate suppression, never event kinds or reference IDs.
    intent.id = hex::encode(Sha256::digest(intent.id.as_bytes()));
    let _guard = FILE_LOCK.lock().map_err(|e| e.to_string())?;
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
    let paths = state
        .profile_manager
        .paths_for(&ProfileId::parse(&intent.profile_id).map_err(|e| e.to_string())?);
    let bytes: [u8; 32] = std::fs::read(paths.root.join(PUBLIC_KEY))
        .map_err(|e| e.to_string())?
        .try_into()
        .map_err(|_| "invalid notification encryption key")?;
    let ephemeral = StaticSecret::random_from_rng(rand::rngs::OsRng);
    let key = Zeroizing::new(group_key::generate_group_key());
    let encrypted_key =
        group_key::encrypt_key_for_member(&key, &PublicKey::from(bytes), &ephemeral)
            .map_err(|e| e.to_string())?;
    let plaintext = Zeroizing::new(serde_json::to_vec(&intent).map_err(|e| e.to_string())?);
    let ciphertext = group_key::encrypt_message(&key, &plaintext).map_err(|e| e.to_string())?;
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

pub async fn pending(state: &AppState) -> Result<Option<NotificationOpen>, String> {
    let sealed = {
        let _guard = FILE_LOCK.lock().map_err(|e| e.to_string())?;
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
    let lease = state.profile_operations.admit(&session)?;
    if state.active_id().as_ref().map(|p| p.as_str()) != Some(sealed.routing.profile_id.as_str()) {
        return Ok(Some(sealed.routing));
    }
    let keystore = state.keystore.lock().await;
    let private = secret(
        keystore.as_ref().ok_or("vault is locked")?,
        &sealed.routing.profile_id,
    )?;
    let key = Zeroizing::new(
        group_key::decrypt_key_from_member(
            &sealed.encrypted_key,
            &PublicKey::from(sealed.ephemeral),
            &private,
        )
        .map_err(|e| e.to_string())?,
    );
    let plaintext = Zeroizing::new(
        group_key::decrypt_message(&key, &sealed.ciphertext).map_err(|e| e.to_string())?,
    );
    let intent: NotificationOpen = serde_json::from_slice(&plaintext).map_err(|e| e.to_string())?;
    let mut routing = intent.clone();
    routing.route.clear();
    if routing != sealed.routing || !valid(state, &intent) {
        return Err("invalid notification envelope".into());
    }
    lease.with_current(|| Ok(Some(intent)))
}

pub fn consume(state: &AppState, lease: &ProfileLease, id: &str) -> Result<(), String> {
    lease.with_current(|| {
        let _guard = FILE_LOCK.lock().map_err(|e| e.to_string())?;
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
            return Err("notification belongs to another profile".into());
        }
        opens.pending = None;
        opens.consumed.push(id.into());
        if opens.consumed.len() > 256 {
            opens.consumed.remove(0);
        }
        save(state, &opens)
    })
}

/// Local banner extras are produced by this app; remote data cannot use this adapter.
pub fn local_tap(
    state: &AppState,
    extra: &serde_json::Map<String, serde_json::Value>,
) -> Result<Option<NotificationOpen>, String> {
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
