//! The companion protocol on the shared iroh endpoint ([`super::ALPN`]).
//!
//! Each exchange is one bidirectional QUIC stream carrying one JSON
//! message each way. The phone dials the desktop and sends its hello;
//! after the desktop has stored it, the connection stays open: the phone
//! sends camera windows on it, and the desktop asks the phone, on the same
//! connection, to sign the terminal commitment root and, at issuance, for
//! evidence over a credential's binding digest ([`CompanionHub::request_binding`],
//! [`PairedAttester`]).
//!
//! [`CompanionHub`] is both ends. The desktop registers an offer and waits
//! for the join ([`CompanionHub::offer`] / [`CompanionHub::take_join`]);
//! the protocol handler judges the hello with [`super::accept_hello`] and
//! hands it over, and the caller stores it and answers. The phone side is
//! [`CompanionHub::join`].

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ed25519_dalek::SigningKey;
use iroh::endpoint::Connection;
use iroh::protocol::{AcceptError, ProtocolHandler};
use iroh::Endpoint;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;

use super::{accept_hello, answer_offer, build_hello, Accepted, CompanionHello, CompanionOffer};
use crate::sentinel::attestation::native::Captured;
use crate::sentinel::attestation::{AttestationPayload, TrustConfig};
use crate::sentinel::camera_budget::BenchmarkResult;
use crate::sentinel::scoring::CameraFeatures;

/// Largest message either side reads.
const MAX_MESSAGE: usize = 256 * 1024;
/// How long the protocol handler waits for the hello to be stored, and the
/// desktop for the phone's end assertion.
const REPLY_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum PhoneMessage {
    Hello(Box<CompanionHello>),
    Camera {
        session_id: String,
        camera: CameraFeatures,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum DesktopReply {
    Accepted { assurance_level: String },
    Received,
    Refused(String),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum DesktopRequest {
    AssertRoot {
        session_id: String,
        root: String,
    },
    /// Evidence over a credential's binding digest (hex).
    Bind {
        session_id: String,
        digest_hex: String,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum PhoneReply {
    Assertion(Option<String>),
    Refused(String),
}

/// A hello the protocol accepted, waiting for the session to store it.
/// Answer with [`Joined::answer`].
pub struct Joined {
    pub accepted: Accepted,
    reply: oneshot::Sender<Result<String, String>>,
    /// The phone link, put in place when the pairing is stored.
    link: (Arc<Mutex<HubState>>, String, DesktopLink),
}

impl Joined {
    /// Answer the phone with the result of storing the pairing:
    /// `Ok(assurance_level)` or `Err(reason)`. On success the phone link is
    /// in place before this returns, so the session's next read of the
    /// phone's role sees it.
    pub fn answer(self, stored: Result<String, String>) {
        let (state, session_id, link) = self.link;
        if stored.is_ok() {
            // A poisoned hub only holds transient pairing state.
            let mut state = state.lock().unwrap_or_else(|e| e.into_inner());
            state.links.insert(session_id, link);
        }
        let _ = self.reply.send(stored);
    }
}

/// Platform capture on the phone (blocking): `native::capture` in the app.
pub type Capture = Box<dyn FnOnce([u8; 32]) -> Result<Option<Captured>, String> + Send>;
/// Phone-side App Attest assertion over a root (blocking):
/// `native::assert_commitment` in the app.
pub type Asserter = Arc<dyn Fn(&str, &str) -> Result<Option<String>, String> + Send + Sync>;
/// Phone-side evidence over a binding digest, given the session's App Attest
/// key id when there is one (blocking): `native::bind_digest` in the app.
pub type Binder =
    Arc<dyn Fn(Option<&str>, &[u8; 32]) -> Result<Option<String>, String> + Send + Sync>;

/// What the phone signs with for as long as its link stays up.
#[derive(Clone)]
pub struct PhoneSigner {
    /// The terminal commitment root, at session end.
    pub assert_root: Asserter,
    /// A credential's binding digest, at issuance.
    pub bind_digest: Binder,
}

/// How long the desktop waits for the phone's evidence at issuance. Issuance
/// holds the database while it waits, so this is shorter than a reply to the
/// learner's own actions.
const BINDING_TIMEOUT: Duration = Duration::from_secs(15);
/// The reason issuance gives when no phone is linked to the session.
const NO_PHONE: &str = "the paired phone is no longer linked to this session";

type TrustSource = Arc<dyn Fn() -> TrustConfig + Send + Sync>;
type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;

struct Pending {
    offer: CompanionOffer,
    waiter: Option<oneshot::Sender<Joined>>,
    receiver: Option<oneshot::Receiver<Joined>>,
}

struct DesktopLink {
    conn: Connection,
    remote: String,
    camera: bool,
    benchmark: Option<BenchmarkResult>,
    window: Option<CameraFeatures>,
}

struct PhoneLink {
    session_id: String,
    conn: Connection,
}

#[derive(Default)]
struct HubState {
    offers: HashMap<String, Pending>,
    links: HashMap<String, DesktopLink>,
    phone: Option<PhoneLink>,
}

/// The phone's role in a paired session, as the desktop sees it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompanionRole {
    /// The phone is the session's camera.
    pub camera: bool,
    /// The phone's camera benchmark, when it reported one.
    pub benchmark: Option<BenchmarkResult>,
}

/// Both ends of the companion protocol for this app instance.
#[derive(Clone)]
pub struct CompanionHub {
    state: Arc<Mutex<HubState>>,
    trust: TrustSource,
    now: Clock,
}

impl std::fmt::Debug for CompanionHub {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompanionHub").finish_non_exhaustive()
    }
}

impl Default for CompanionHub {
    fn default() -> Self {
        Self::new()
    }
}

impl CompanionHub {
    /// Trust material from the embedded network profile, wall-clock time.
    pub fn new() -> Self {
        Self::with(
            Arc::new(TrustConfig::from_profile),
            Arc::new(|| chrono::Utc::now().timestamp()),
        )
    }

    /// Injected trust material and clock (tests).
    pub fn with(trust: TrustSource, now: Clock) -> Self {
        Self {
            state: Arc::new(Mutex::new(HubState::default())),
            trust,
            now,
        }
    }

    pub fn now(&self) -> i64 {
        (self.now)()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HubState> {
        // A poisoned hub only holds transient pairing state.
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The handler to register on the router under [`super::ALPN`].
    pub fn protocol(&self) -> CompanionProtocol {
        CompanionProtocol { hub: self.clone() }
    }

    // ---- desktop -------------------------------------------------------

    /// Register an offer, replacing any earlier one for its session.
    pub fn offer(&self, offer: CompanionOffer) {
        let (tx, rx) = oneshot::channel();
        self.lock().offers.insert(
            offer.session_id.clone(),
            Pending {
                offer,
                waiter: Some(tx),
                receiver: Some(rx),
            },
        );
    }

    /// The receiver for the session's join; `None` without an offer or once
    /// taken.
    pub fn take_join(&self, session_id: &str) -> Option<oneshot::Receiver<Joined>> {
        self.lock()
            .offers
            .get_mut(session_id)
            .and_then(|p| p.receiver.take())
    }

    /// Forget the session's offer and close its phone link.
    pub fn close(&self, session_id: &str) {
        let mut state = self.lock();
        state.offers.remove(session_id);
        if let Some(link) = state.links.remove(session_id) {
            link.conn.close(0u32.into(), b"session closed");
        }
    }

    /// Forget the session's offer now and close its phone link after
    /// `delay`, so issuance after the session ends can still ask the phone
    /// for evidence ([`Self::request_binding`]). Needs a Tokio runtime.
    pub fn close_after(&self, session_id: &str, delay: Duration) {
        self.lock().offers.remove(session_id);
        let (hub, session_id) = (self.clone(), session_id.to_string());
        tokio::spawn(async move {
            tokio::time::sleep(delay).await;
            hub.close(&session_id);
        });
    }

    /// The paired phone's role, if a phone is linked to the session.
    pub fn role(&self, session_id: &str) -> Option<CompanionRole> {
        self.lock().links.get(session_id).map(|link| CompanionRole {
            camera: link.camera,
            benchmark: link.benchmark,
        })
    }

    /// The paired phone's role once the learner answered companion consent
    /// at profile selection. Declined, the phone is dropped from the session
    /// (offer withdrawn, link closed), so neither its benchmark nor its
    /// camera windows reach selection or any snapshot.
    pub fn consented_role(&self, session_id: &str, consented: bool) -> Option<CompanionRole> {
        if !consented {
            self.close(session_id);
        }
        self.role(session_id)
    }

    /// The camera scores the phone sent since the last call, merged.
    pub fn take_camera_window(&self, session_id: &str) -> Option<CameraFeatures> {
        self.lock()
            .links
            .get_mut(session_id)
            .and_then(|link| link.window.take())
    }

    /// Ask the session's phone to sign `root` with its attested key.
    pub async fn request_assertion(
        &self,
        session_id: &str,
        root: &str,
    ) -> Result<Option<String>, String> {
        let conn = self
            .lock()
            .links
            .get(session_id)
            .map(|link| link.conn.clone())
            .ok_or("no phone is paired with this session")?;
        let request = DesktopRequest::AssertRoot {
            session_id: session_id.into(),
            root: root.into(),
        };
        let reply = tokio::time::timeout(REPLY_TIMEOUT, exchange(&conn, &request))
            .await
            .map_err(|_| "the phone did not answer in time".to_string())??;
        match reply {
            PhoneReply::Assertion(assertion) => Ok(assertion),
            PhoneReply::Refused(why) => Err(why),
        }
    }

    /// Ask the session's phone for evidence over a credential's binding
    /// digest: on an iPhone an assertion from its attested key whose client
    /// data is the digest, on Android a token with the digest as nonce.
    pub async fn request_binding(
        &self,
        session_id: &str,
        digest: &[u8; 32],
    ) -> Result<String, String> {
        let conn = self
            .lock()
            .links
            .get(session_id)
            .map(|link| link.conn.clone())
            .ok_or(NO_PHONE)?;
        let request = DesktopRequest::Bind {
            session_id: session_id.into(),
            digest_hex: hex::encode(digest),
        };
        let reply = tokio::time::timeout(BINDING_TIMEOUT, exchange(&conn, &request))
            .await
            .map_err(|_| "the paired phone did not answer in time".to_string())??;
        match reply {
            PhoneReply::Assertion(Some(proof)) => Ok(proof),
            PhoneReply::Assertion(None) => Err("the paired phone cannot attest".into()),
            PhoneReply::Refused(why) => Err(why),
        }
    }

    async fn on_hello(
        &self,
        hello: CompanionHello,
        remote: &str,
        conn: &Connection,
    ) -> DesktopReply {
        // Single use: whatever happens, this offer is spent.
        let Some(mut pending) = self.lock().offers.remove(&hello.session_id) else {
            return DesktopReply::Refused("no companion code is open for this session".into());
        };
        let accepted =
            match accept_hello(&pending.offer, &hello, remote, &(self.trust)(), self.now()) {
                Ok(accepted) => accepted,
                Err(why) => return DesktopReply::Refused(why),
            };
        let Some(waiter) = pending.waiter.take() else {
            return DesktopReply::Refused("the desktop is not waiting for a phone".into());
        };
        let link = DesktopLink {
            conn: conn.clone(),
            remote: remote.into(),
            camera: pending.offer.camera,
            benchmark: accepted.camera_benchmark,
            window: None,
        };
        let (reply, answer) = oneshot::channel();
        let joined = Joined {
            accepted,
            reply,
            link: (self.state.clone(), hello.session_id.clone(), link),
        };
        if waiter.send(joined).is_err() {
            return DesktopReply::Refused("the desktop stopped waiting for a phone".into());
        }
        match tokio::time::timeout(REPLY_TIMEOUT, answer).await {
            // The link went in with the answer (`Joined::answer`).
            Ok(Ok(Ok(assurance_level))) => DesktopReply::Accepted { assurance_level },
            Ok(Ok(Err(why))) => DesktopReply::Refused(why),
            _ => {
                // An answer that came too late may have put the link in.
                let mut state = self.lock();
                if state
                    .links
                    .get(&hello.session_id)
                    .is_some_and(|link| link.remote == remote)
                {
                    state.links.remove(&hello.session_id);
                }
                DesktopReply::Refused("the desktop did not store the pairing".into())
            }
        }
    }

    fn on_camera(&self, session_id: &str, remote: &str, camera: CameraFeatures) -> DesktopReply {
        let mut state = self.lock();
        match state.links.get_mut(session_id) {
            Some(link) if link.remote == remote && link.camera => {
                link.window = Some(match link.window.take() {
                    Some(earlier) => merge_windows(earlier, camera),
                    None => camera,
                });
                DesktopReply::Received
            }
            Some(link) if link.remote == remote => {
                DesktopReply::Refused("this phone is not the session's camera".into())
            }
            _ => DesktopReply::Refused("this phone is not paired with the session".into()),
        }
    }

    fn on_disconnect(&self, remote: &str) {
        self.lock().links.retain(|_, link| link.remote != remote);
    }

    // ---- phone ---------------------------------------------------------

    /// Join the desktop session in `offer` as the learner whose key is
    /// `learner_key`. Attests with `capture`; serves end assertions and
    /// binding evidence with `signer` for as long as the link stays up.
    /// Returns the assurance the desktop recorded.
    pub async fn join(
        &self,
        endpoint: &Endpoint,
        offer: &CompanionOffer,
        learner_key: &SigningKey,
        capture: Capture,
        signer: PhoneSigner,
        camera_benchmark: Option<BenchmarkResult>,
    ) -> Result<String, String> {
        let phone = endpoint.id().to_string();
        let did = alexandria_verify::did::derive_did_key(learner_key);
        let nonce = answer_offer(offer, did.as_str(), &phone, self.now())?;
        let captured = tokio::task::spawn_blocking(move || capture(nonce))
            .await
            .map_err(|e| format!("attestation worker failed: {e}"))??
            .ok_or("this device cannot attest; pair an iPhone or Android phone")?;
        let key_id = match &captured.payload {
            AttestationPayload::AppAttest { key_id_b64, .. } => Some(key_id_b64.clone()),
            AttestationPayload::PlayIntegrity { .. } => None,
        };
        let hello = build_hello(
            offer,
            learner_key,
            &phone,
            &nonce,
            captured,
            &crate::clock::now_rfc3339(),
            camera_benchmark,
        );
        let conn = endpoint
            .connect(offer.desktop.clone(), super::ALPN)
            .await
            .map_err(|e| format!("could not reach the desktop: {e}"))?;
        let reply: DesktopReply = exchange(&conn, &PhoneMessage::Hello(Box::new(hello))).await?;
        let assurance_level = match reply {
            DesktopReply::Accepted { assurance_level } => assurance_level,
            DesktopReply::Refused(why) => return Err(why),
            DesktopReply::Received => return Err("unexpected reply from the desktop".into()),
        };
        if let Some(old) = self.lock().phone.replace(PhoneLink {
            session_id: offer.session_id.clone(),
            conn: conn.clone(),
        }) {
            old.conn.close(0u32.into(), b"joined another session");
        }
        tokio::spawn(serve_desktop(
            conn,
            offer.session_id.clone(),
            key_id,
            signer,
        ));
        Ok(assurance_level)
    }

    /// Phone: send one window of camera scores to the paired desktop.
    pub async fn send_camera(&self, camera: CameraFeatures) -> Result<(), String> {
        let (conn, session_id) = {
            let state = self.lock();
            let link = state.phone.as_ref().ok_or("this phone is not paired")?;
            (link.conn.clone(), link.session_id.clone())
        };
        match exchange(&conn, &PhoneMessage::Camera { session_id, camera }).await? {
            DesktopReply::Received => Ok(()),
            DesktopReply::Refused(why) => Err(why),
            DesktopReply::Accepted { .. } => Err("unexpected reply from the desktop".into()),
        }
    }

    /// Phone: the session this phone is paired with, if any.
    pub fn joined_session(&self) -> Option<String> {
        self.lock().phone.as_ref().map(|l| l.session_id.clone())
    }

    /// Phone: drop the link.
    pub fn leave(&self) {
        if let Some(link) = self.lock().phone.take() {
            link.conn.close(0u32.into(), b"left");
        }
    }
}

/// Sum counts, keep the latest readings.
fn merge_windows(a: CameraFeatures, b: CameraFeatures) -> CameraFeatures {
    let mean = |x: Option<f64>, nx: u32, y: Option<f64>, ny: u32| match (x, y) {
        (Some(x), Some(y)) if nx + ny > 0 => {
            Some((x * nx as f64 + y * ny as f64) / (nx + ny) as f64)
        }
        (x, y) => y.or(x),
    };
    CameraFeatures {
        device_virtual: b.device_virtual.or(a.device_virtual),
        face_present: b.face_present.or(a.face_present),
        face_count: b.face_count.or(a.face_count),
        face_consistency: b.face_consistency.or(a.face_consistency),
        face_similarity: b.face_similarity.or(a.face_similarity),
        face_match: b.face_match.or(a.face_match),
        consecutive_no_face: b.consecutive_no_face,
        face_checks: a.face_checks + b.face_checks,
        face_absent_checks: a.face_absent_checks + b.face_absent_checks,
        gaze_checks: a.gaze_checks + b.gaze_checks,
        gaze_offscreen_checks: a.gaze_offscreen_checks + b.gaze_offscreen_checks,
        gaze_occluded_checks: a.gaze_occluded_checks + b.gaze_occluded_checks,
        gaze_down_glances: a.gaze_down_glances + b.gaze_down_glances,
        liveness_checks: a.liveness_checks + b.liveness_checks,
        liveness_spoof_checks: a.liveness_spoof_checks + b.liveness_spoof_checks,
        liveness_real_prob: mean(
            a.liveness_real_prob,
            a.liveness_checks,
            b.liveness_real_prob,
            b.liveness_checks,
        ),
    }
}

/// Phone: answer the desktop's requests on the link until it closes.
async fn serve_desktop(
    conn: Connection,
    session_id: String,
    key_id: Option<String>,
    signer: PhoneSigner,
) {
    while let Ok((mut send, mut recv)) = conn.accept_bi().await {
        let Ok(request) = read_message::<DesktopRequest>(&mut recv).await else {
            continue;
        };
        let asked = match &request {
            DesktopRequest::AssertRoot { session_id, .. }
            | DesktopRequest::Bind { session_id, .. } => session_id,
        };
        let reply = if *asked != session_id {
            PhoneReply::Refused("not the session this phone joined".into())
        } else {
            match request {
                DesktopRequest::AssertRoot { root, .. } => match &key_id {
                    None => PhoneReply::Assertion(None),
                    Some(key_id) => {
                        let (asserter, key_id) = (signer.assert_root.clone(), key_id.clone());
                        signed(move || asserter(&key_id, &root)).await
                    }
                },
                DesktopRequest::Bind { digest_hex, .. } => {
                    match hex::decode(&digest_hex)
                        .ok()
                        .and_then(|d| <[u8; 32]>::try_from(d).ok())
                    {
                        None => PhoneReply::Refused("the binding digest is malformed".into()),
                        Some(digest) => {
                            let (binder, key_id) = (signer.bind_digest.clone(), key_id.clone());
                            signed(move || binder(key_id.as_deref(), &digest)).await
                        }
                    }
                }
            }
        };
        let _ = write_message(&mut send, &reply).await;
    }
}

/// Run a blocking platform signature off the link's task.
async fn signed(
    sign: impl FnOnce() -> Result<Option<String>, String> + Send + 'static,
) -> PhoneReply {
    match tokio::task::spawn_blocking(sign).await {
        Ok(Ok(proof)) => PhoneReply::Assertion(proof),
        Ok(Err(why)) => PhoneReply::Refused(why),
        Err(e) => PhoneReply::Refused(format!("assertion worker failed: {e}")),
    }
}

/// Issuance's evidence port on a desktop: a paired phone's session asks the
/// phone over the companion link ([`CompanionHub::request_binding`]); any
/// other session uses this device's own platform. Called from the database
/// thread, which it blocks until the phone answers or the wait times out.
pub struct PairedAttester {
    pub hub: CompanionHub,
    pub runtime: tokio::runtime::Handle,
}

impl crate::sentinel::attestation::BindingAttester for PairedAttester {
    fn bind(
        &self,
        session_id: &str,
        record: &crate::sentinel::attestation::DeviceAttestation,
        digest: &[u8; 32],
    ) -> Result<String, String> {
        if record.companion.is_none() {
            return crate::sentinel::attestation::NativeAttester.bind(session_id, record, digest);
        }
        let (hub, session_id, digest) = (self.hub.clone(), session_id.to_string(), *digest);
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.spawn(async move {
            let _ = tx.send(hub.request_binding(&session_id, &digest).await);
        });
        rx.recv_timeout(BINDING_TIMEOUT + Duration::from_secs(1))
            .map_err(|_| "the paired phone did not answer in time".to_string())?
    }
}

/// The desktop's handler for [`super::ALPN`] connections.
#[derive(Debug, Clone)]
pub struct CompanionProtocol {
    hub: CompanionHub,
}

impl ProtocolHandler for CompanionProtocol {
    async fn accept(&self, conn: Connection) -> Result<(), AcceptError> {
        let remote = conn.remote_id().to_string();
        while let Ok((mut send, mut recv)) = conn.accept_bi().await {
            let reply = match read_message::<PhoneMessage>(&mut recv).await {
                Ok(PhoneMessage::Hello(hello)) => self.hub.on_hello(*hello, &remote, &conn).await,
                Ok(PhoneMessage::Camera { session_id, camera }) => {
                    self.hub.on_camera(&session_id, &remote, camera)
                }
                Err(why) => DesktopReply::Refused(why),
            };
            let _ = write_message(&mut send, &reply).await;
        }
        self.hub.on_disconnect(&remote);
        Ok(())
    }
}

async fn exchange<Q: Serialize, A: DeserializeOwned>(
    conn: &Connection,
    q: &Q,
) -> Result<A, String> {
    let (mut send, mut recv) = conn
        .open_bi()
        .await
        .map_err(|e| format!("companion link: {e}"))?;
    write_message(&mut send, q).await?;
    read_message(&mut recv).await
}

async fn write_message<T: Serialize>(
    send: &mut iroh::endpoint::SendStream,
    message: &T,
) -> Result<(), String> {
    let bytes = serde_json::to_vec(message).map_err(|e| e.to_string())?;
    send.write_all(&bytes)
        .await
        .map_err(|e| format!("companion link: {e}"))?;
    send.finish().map_err(|e| format!("companion link: {e}"))
}

async fn read_message<T: DeserializeOwned>(
    recv: &mut iroh::endpoint::RecvStream,
) -> Result<T, String> {
    let bytes = recv
        .read_to_end(MAX_MESSAGE)
        .await
        .map_err(|e| format!("companion link: {e}"))?;
    serde_json::from_slice(&bytes).map_err(|e| format!("companion message: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sentinel::attestation::app_attest;
    use crate::sentinel::attestation::{apple_app_id, ASSURANCE_DEVICE_ATTESTED};
    use crate::sentinel::companion::tests::{
        did_of, learner, phone_capture, trust, SESSION, STARTED,
    };
    use alexandria_verify::attestation::test_support::{build_assertion, World};
    use base64::Engine as _;
    use iroh::address_lookup::MemoryLookup;
    use iroh::protocol::Router;
    use iroh::{EndpointAddr, RelayMode, TransportAddr};
    use sha2::{Digest, Sha256};

    const NOW: i64 = 1_760_000_000;

    /// An endpoint on loopback, findable only through the shared book.
    async fn bind(book: &MemoryLookup) -> Endpoint {
        let endpoint = Endpoint::builder(iroh::endpoint::presets::Minimal)
            .relay_mode(RelayMode::Disabled)
            .address_lookup(book.clone())
            .bind_addr("127.0.0.1:0")
            .expect("loopback address")
            .bind()
            .await
            .expect("bind loopback endpoint");
        let socket = endpoint
            .bound_sockets()
            .into_iter()
            .find(|s| s.is_ipv4())
            .expect("an IPv4 socket");
        book.add_endpoint_info(EndpointAddr::from_parts(
            endpoint.id(),
            [TransportAddr::Ip(socket)],
        ));
        endpoint
    }

    fn window(face_checks: u32, liveness: Option<f64>) -> CameraFeatures {
        CameraFeatures {
            face_checks,
            face_present: Some(true),
            liveness_checks: 1,
            liveness_real_prob: liveness,
            ..Default::default()
        }
    }

    /// A desktop with an open offer and a phone that can answer it.
    struct Pair {
        desktop: CompanionHub,
        phone: CompanionHub,
        phone_ep: Endpoint,
        offer: CompanionOffer,
        nonce: [u8; 32],
        world: Arc<World>,
        _router: Router,
    }

    async fn pair(camera: bool) -> Pair {
        let book = MemoryLookup::new();
        let desktop_ep = bind(&book).await;
        let phone_ep = bind(&book).await;
        let offer = CompanionOffer::new(
            SESSION,
            STARTED,
            &did_of(&learner()),
            desktop_ep.addr(),
            camera,
            NOW,
        );
        let nonce =
            answer_offer(&offer, &did_of(&learner()), &phone_ep.id().to_string(), NOW).unwrap();
        let (world, _) = phone_capture(nonce);
        let cfg = trust(&world);
        let desktop = CompanionHub::with(Arc::new(move || cfg.clone()), Arc::new(|| NOW));
        let router = Router::builder(desktop_ep)
            .accept(super::super::ALPN, desktop.protocol())
            .spawn();
        desktop.offer(offer.clone());
        Pair {
            desktop,
            phone: CompanionHub::with(Arc::new(TrustConfig::default), Arc::new(|| NOW)),
            phone_ep,
            offer,
            nonce,
            world: Arc::new(world),
            _router: router,
        }
    }

    impl Pair {
        /// Desktop side: answer the join with `store`.
        fn store_with(&self, store: Result<(), String>) -> tokio::task::JoinHandle<Accepted> {
            let joined = self.desktop.take_join(SESSION).expect("an open offer");
            tokio::spawn(async move {
                let joined = joined.await.expect("the phone joined");
                let accepted = joined.accepted.clone();
                let level = accepted.assurance_level.clone();
                joined.answer(store.map(|_| level));
                accepted
            })
        }

        async fn join(&self, benchmark: Option<BenchmarkResult>) -> Result<String, String> {
            let world = self.world.clone();
            let nonce = self.nonce;
            let (signer, captured) = (world.clone(), {
                use crate::sentinel::attestation::{native::Captured, Platform};
                let b64 = |b: &[u8]| base64::engine::general_purpose::STANDARD.encode(b);
                Captured {
                    platform: Platform::Ios,
                    app_id: apple_app_id(),
                    payload: AttestationPayload::AppAttest {
                        key_id_b64: b64(&world.key_id),
                        attestation_b64: b64(&world.attestation),
                        assertion_b64: None,
                        asserted_root: None,
                    },
                }
            });
            // The attested key's counter moves with every assertion, as
            // `generateAssertion` does.
            let counter = Arc::new(std::sync::atomic::AtomicU32::new(0));
            let next = move || counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
            let (root_signer, root_next) = (signer.clone(), next.clone());
            let signer = PhoneSigner {
                assert_root: Arc::new(move |_key_id, root| {
                    Ok(Some(base64::engine::general_purpose::STANDARD.encode(
                        build_assertion(&root_signer, root.as_bytes(), root_next()),
                    )))
                }),
                bind_digest: Arc::new(move |key_id, digest| {
                    assert!(key_id.is_some(), "an iPhone binds with its session key");
                    Ok(Some(
                        base64::engine::general_purpose::STANDARD.encode(build_assertion(
                            &signer,
                            digest,
                            next(),
                        )),
                    ))
                }),
            };
            self.phone
                .join(
                    &self.phone_ep,
                    &self.offer,
                    &learner(),
                    Box::new(move |n| {
                        assert_eq!(n, nonce, "the phone attests the companion nonce");
                        Ok(Some(captured))
                    }),
                    signer,
                    benchmark,
                )
                .await
        }
    }

    #[tokio::test]
    async fn a_phone_joins_over_iroh_feeds_camera_scores_and_signs_the_terminal_root() {
        let p = pair(true).await;
        let stored = p.store_with(Ok(()));
        assert!(
            p.desktop.take_join(SESSION).is_none(),
            "one waiter per offer"
        );
        let benchmark = BenchmarkResult {
            detect_ms: 9.0,
            liveness_ms: 7.0,
            track_ms: 0.1,
        };
        assert_eq!(
            p.join(Some(benchmark)).await.unwrap(),
            ASSURANCE_DEVICE_ATTESTED
        );
        let accepted = stored.await.unwrap();
        assert_eq!(
            accepted.record.companion.as_ref().unwrap().phone_endpoint,
            p.phone_ep.id().to_string(),
            "bound to the endpoint the connection authenticated"
        );
        assert_eq!(p.phone.joined_session().as_deref(), Some(SESSION));
        assert_eq!(
            p.desktop.role(SESSION),
            Some(CompanionRole {
                camera: true,
                benchmark: Some(benchmark),
            })
        );

        // Camera scores arrive per window; two sends before a snapshot merge.
        p.phone.send_camera(window(3, Some(0.9))).await.unwrap();
        p.phone.send_camera(window(2, Some(0.6))).await.unwrap();
        let merged = p.desktop.take_camera_window(SESSION).unwrap();
        assert_eq!(merged.face_checks, 5);
        assert_eq!(merged.liveness_checks, 2);
        assert!((merged.liveness_real_prob.unwrap() - 0.75).abs() < 1e-9);
        assert!(p.desktop.take_camera_window(SESSION).is_none());

        // The phone signs the desktop's terminal root with its attested key.
        let assertion = p
            .desktop
            .request_assertion(SESSION, "terminal-root")
            .await
            .unwrap()
            .expect("an iOS phone asserts");
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(assertion)
            .unwrap();
        let public_key = p
            .world
            .leaf_signing_key
            .verifying_key()
            .to_encoded_point(false)
            .as_bytes()
            .to_vec();
        let app_id_hash: [u8; 32] = Sha256::digest(apple_app_id().as_bytes()).into();
        app_attest::verify_assertion(&bytes, b"terminal-root", &public_key, &app_id_hash, 0)
            .unwrap();

        // The offer was single use.
        let err = p.join(None).await.unwrap_err();
        assert!(err.contains("no companion code"), "{err}");

        // Closing the session drops the link.
        p.desktop.close(SESSION);
        assert!(p.desktop.role(SESSION).is_none());
        assert!(p.desktop.request_assertion(SESSION, "r").await.is_err());
    }

    #[tokio::test]
    async fn the_phone_answers_a_binding_request_with_evidence_over_the_digest() {
        let p = pair(false).await;
        let _stored = p.store_with(Ok(()));
        p.join(None).await.unwrap();

        let digest = [9u8; 32];
        let proof = p.desktop.request_binding(SESSION, &digest).await.unwrap();
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(proof)
            .unwrap();
        let public_key = p
            .world
            .leaf_signing_key
            .verifying_key()
            .to_encoded_point(false)
            .as_bytes()
            .to_vec();
        let app_id_hash: [u8; 32] = Sha256::digest(apple_app_id().as_bytes()).into();
        // Client data is the 32 digest bytes, signed by the session's key.
        app_attest::verify_assertion(&bytes, &digest, &public_key, &app_id_hash, 0).unwrap();

        // Once the link is closed there is no phone to ask.
        p.desktop.close(SESSION);
        let err = p
            .desktop
            .request_binding(SESSION, &digest)
            .await
            .unwrap_err();
        assert!(err.contains("no longer linked"), "{err}");
    }

    /// A desktop session paired with `accepted`, its profile chosen with
    /// companion consent, ended clean.
    fn finished_paired_session(accepted: &Accepted) -> crate::db::Database {
        use crate::sentinel::scoring::Capabilities;
        use crate::sentinel::trust_profile::{select_for_session, ProfileRequest};
        let db = crate::db::Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let conn = db.conn();
        conn.execute(
            "INSERT INTO integrity_sessions (id, status, purpose, started_at)
             VALUES (?1, 'active', 'assessment', ?2)",
            rusqlite::params![SESSION, STARTED],
        )
        .unwrap();
        crate::settings::SettingsStore::set(
            conn,
            crate::settings::registry::keys::IDENTITY_LOCAL_DID,
            did_of(&learner()),
        )
        .unwrap();
        super::super::session::store_pairing(conn, SESSION, accepted).unwrap();
        let consent = ProfileRequest {
            consent: Capabilities {
                companion_phone: true,
                ..Default::default()
            },
            ..Default::default()
        };
        select_for_session(conn, SESSION, &consent, None).unwrap();
        conn.execute(
            "UPDATE integrity_sessions SET status = 'completed', ended_at = ?2 WHERE id = ?1",
            rusqlite::params![SESSION, crate::credentials::fixtures::NOW],
        )
        .unwrap();
        db
    }

    /// Issue the learner's credential on `db`'s session, from the database
    /// thread's side: blocking, outside the async context.
    async fn issue_paired(
        db: crate::db::Database,
        desktop: &CompanionHub,
        world: &World,
    ) -> crate::credentials::Issued {
        use crate::credentials::{fixtures, issue_with, EvidenceSource, Issuer};
        let attester = PairedAttester {
            hub: desktop.clone(),
            runtime: tokio::runtime::Handle::current(),
        };
        let trust = trust(world);
        tokio::task::spawn_blocking(move || {
            let learner_did = crate::crypto::did::Did(did_of(&learner()));
            let mut req = fixtures::sample_request(learner_did);
            req.integrity_session_id = Some(SESSION.into());
            issue_with(
                db.conn(),
                &Issuer::from_key(learner()),
                &req,
                fixtures::NOW,
                &EvidenceSource {
                    trust,
                    attester: &attester,
                },
            )
            .unwrap()
        })
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn a_paired_desktop_issues_a_credential_bound_by_the_phone_over_the_link() {
        use crate::credentials::EvidenceBinding;
        use alexandria_verify::attestation::{verify_evidence, AttestationOutcome, Platform};
        let p = pair(false).await;
        let stored = p.store_with(Ok(()));
        p.join(None).await.unwrap();
        let db = finished_paired_session(&stored.await.unwrap());

        let issued = issue_paired(db, &p.desktop, &p.world).await;
        assert_eq!(issued.evidence, EvidenceBinding::Credential);
        assert_eq!(
            verify_evidence(&issued.credential, &trust(&p.world).inputs()),
            AttestationOutcome::VerifiedCredentialBound {
                platform: Platform::Ios
            }
        );
    }

    #[tokio::test]
    async fn once_the_phone_link_closed_a_paired_desktop_issues_session_bound() {
        use crate::credentials::EvidenceBinding;
        use alexandria_verify::attestation::{verify_evidence, AttestationOutcome, Platform};
        let p = pair(false).await;
        let stored = p.store_with(Ok(()));
        p.join(None).await.unwrap();
        let db = finished_paired_session(&stored.await.unwrap());
        p.desktop.close(SESSION);

        let issued = issue_paired(db, &p.desktop, &p.world).await;
        assert_eq!(
            issued.evidence,
            EvidenceBinding::Session {
                reason: NO_PHONE.into()
            }
        );
        assert_eq!(
            verify_evidence(&issued.credential, &trust(&p.world).inputs()),
            AttestationOutcome::VerifiedSessionBound {
                platform: Platform::Ios
            }
        );
    }

    #[tokio::test]
    async fn a_link_kept_for_issuance_answers_until_its_wait_runs_out() {
        let p = pair(false).await;
        let _stored = p.store_with(Ok(()));
        p.join(None).await.unwrap();
        p.desktop.close_after(SESSION, Duration::from_millis(300));
        // Monitoring has ended; grading can still ask the phone.
        assert!(p.desktop.request_binding(SESSION, &[1; 32]).await.is_ok());
        tokio::time::sleep(Duration::from_millis(600)).await;
        assert!(p.desktop.role(SESSION).is_none());
        assert!(p.desktop.request_binding(SESSION, &[1; 32]).await.is_err());
    }

    #[tokio::test]
    async fn the_phone_link_is_in_place_as_soon_as_the_desktop_has_stored_the_pairing() {
        let p = pair(true).await;
        let joined = p.desktop.take_join(SESSION).expect("an open offer");
        let desktop = p.desktop.clone();
        // As `integrity_companion_await` does: store, answer, then read the
        // phone's role, with nothing in between.
        let seen = tokio::spawn(async move {
            let joined = joined.await.expect("the phone joined");
            let level = joined.accepted.assurance_level.clone();
            joined.answer(Ok(level));
            desktop.role(SESSION)
        });
        p.join(None).await.unwrap();
        assert!(seen.await.unwrap().is_some_and(|role| role.camera));
    }

    #[tokio::test]
    async fn declining_companion_consent_drops_the_phone_so_it_supplies_nothing() {
        let p = pair(true).await;
        let _stored = p.store_with(Ok(()));
        let benchmark = BenchmarkResult {
            detect_ms: 9.0,
            liveness_ms: 7.0,
            track_ms: 0.1,
        };
        p.join(Some(benchmark)).await.unwrap();
        p.phone.send_camera(window(3, Some(0.9))).await.unwrap();
        // The learner declines: no benchmark and no camera window from the
        // phone reach selection or any snapshot.
        assert_eq!(p.desktop.consented_role(SESSION, false), None);
        assert_eq!(p.desktop.role(SESSION), None);
        assert!(p.desktop.take_camera_window(SESSION).is_none());
        assert!(p.phone.send_camera(window(1, None)).await.is_err());
    }

    #[tokio::test]
    async fn consenting_keeps_the_phone_as_the_sessions_camera() {
        let p = pair(true).await;
        let _stored = p.store_with(Ok(()));
        p.join(None).await.unwrap();
        assert!(p
            .desktop
            .consented_role(SESSION, true)
            .is_some_and(|role| role.camera));
        assert!(p.desktop.role(SESSION).is_some());
    }

    #[tokio::test]
    async fn a_phone_that_is_not_the_camera_cannot_feed_camera_scores() {
        let p = pair(false).await;
        let _stored = p.store_with(Ok(()));
        p.join(None).await.unwrap();
        let err = p.phone.send_camera(window(1, None)).await.unwrap_err();
        assert!(err.contains("not the session's camera"), "{err}");
        assert!(p.desktop.take_camera_window(SESSION).is_none());
    }

    #[tokio::test]
    async fn a_desktop_that_refuses_to_store_the_pairing_leaves_no_link() {
        let p = pair(true).await;
        let _stored = p.store_with(Err(
            "a phone pairs before the trust profile is chosen".into()
        ));
        let err = p.join(None).await.unwrap_err();
        assert!(err.contains("before the trust profile"), "{err}");
        assert!(p.desktop.role(SESSION).is_none());
        assert!(p.phone.joined_session().is_none());
    }
}
