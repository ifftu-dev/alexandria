//! cmi5 sessions: the app as the launching system for a cmi5 package.
//!
//! - **Launch** ([`launch`]). Before a unit starts, the app writes the
//!   `LMS.LaunchData` state document and the learner preferences document,
//!   records `launched`, opens the session's endpoint and admits it in the
//!   launch's content policy. The unit gets the launch parameters cmi5
//!   defines merged into its URL: the endpoint, a one-use fetch URL for its
//!   authorisation token, its actor, the registration and its activity id.
//! - **Endpoint.** A per-session HTTP/1.1 listener on `127.0.0.1` (the
//!   transport ticket 23 settled), alive only while the session is open. It
//!   answers only requests from the package origin, to its own address,
//!   carrying the session's token, and implements only what cmi5 needs:
//!   statements, activity state, agent and activity profiles, about. It is
//!   not a conformant record store.
//! - **Session rules.** Every statement the unit sends is checked by
//!   [`alexandria_studio::xapi::cmi5::check`]; one that breaks a rule is
//!   refused with 403 and nothing in its batch is stored.
//! - **Completion.** A unit's `completed`, `passed` and `failed` statements
//!   decide whether it met its moveOn criteria; the app records `satisfied`
//!   for each block and the course whose units all have, and the package
//!   element is complete when the course is.
//! - **End** ([`end`]). Closing the session stops the endpoint; a session
//!   the unit never terminated is recorded `abandoned`. A session left open
//!   when the app stopped is abandoned at the attempt's next launch.
//! - **Identity.** The unit's actor is the pseudonymous learner identifier
//!   every package's content receives, and its registration is derived from
//!   the learner's key. Statements stay on the device in that form; what the
//!   learner's records keep is rebuilt for each recipient when sent.

mod endpoint;
mod listener;
mod store;

#[cfg(test)]
mod conformance;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use alexandria_studio::package::progress;
use alexandria_studio::package::{LaunchKind, LaunchMethod, MoveOn, Standard};
use alexandria_studio::xapi::cmi5::{LaunchMode, SessionFacts, SessionRecord};
use chrono::{DateTime, Utc};
use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use rand::RngCore;

use super::{in_transaction, AttemptError};
use crate::domain::package_attempt::{Cmi5Launch, PackageAttemptProgress};
use crate::package_serving::{self, Launches};
use crate::packages::PackageDb;

pub use listener::revive;

/// The page a unit returns to when it exits, under the launch prefix.
pub const RETURN_PAGE: &str = "_alexandria/returned.html";

/// The open sessions of this profile. Held in the app state; emptied, with
/// every endpoint stopped, when the profile stops.
#[derive(Default)]
pub struct Cmi5Sessions {
    live: Mutex<HashMap<String, Arc<Live>>>,
}

impl Cmi5Sessions {
    pub fn new() -> Self {
        Self::default()
    }

    /// Stops every endpoint, as the profile stops. The sessions stay open in
    /// the database and are abandoned at the next launch.
    pub fn close_all(&self) {
        let live: Vec<_> = self
            .live
            .lock()
            .expect("cmi5 session lock poisoned")
            .drain()
            .map(|(_, live)| live)
            .collect();
        for session in live {
            session.close();
        }
    }

    /// Whether a session is open.
    pub fn is_open(&self, session_id: &str) -> bool {
        self.get(session_id).is_some()
    }

    fn get(&self, session_id: &str) -> Option<Arc<Live>> {
        self.live
            .lock()
            .expect("cmi5 session lock poisoned")
            .get(session_id)
            .cloned()
    }

    fn insert(&self, live: Arc<Live>) {
        self.live
            .lock()
            .expect("cmi5 session lock poisoned")
            .insert(live.id.clone(), live);
    }

    fn remove(&self, session_id: &str) -> Option<Arc<Live>> {
        self.live
            .lock()
            .expect("cmi5 session lock poisoned")
            .remove(session_id)
    }

    fn all(&self) -> Vec<Arc<Live>> {
        self.live
            .lock()
            .expect("cmi5 session lock poisoned")
            .values()
            .cloned()
            .collect()
    }

    fn of_attempt(&self, attempt_id: &str) -> Vec<String> {
        self.live
            .lock()
            .expect("cmi5 session lock poisoned")
            .values()
            .filter(|live| live.attempt_id == attempt_id)
            .map(|live| live.id.clone())
            .collect()
    }
}

/// One open session, as the endpoint serves it. Its state lives here, never
/// in the socket, so the listener can be dropped and bound again.
struct Live {
    id: String,
    attempt_id: String,
    /// The unit's identifier in the course structure.
    unit_id: String,
    package_hash: String,
    prefix: String,
    facts: SessionFacts,
    token: String,
    fetch_code: String,
    port: u16,
    fetch_used: Mutex<bool>,
    record: Mutex<SessionRecord>,
    /// Held while statements are checked and stored, so a session's
    /// statements are taken one batch at a time, in order.
    writing: tokio::sync::Mutex<()>,
    /// Set when the session ends; the endpoint answers nothing after it.
    closed: Mutex<bool>,
    listener: Mutex<Option<listener::Listener>>,
}

impl Live {
    fn close(&self) {
        *self.closed.lock().expect("cmi5 session lock poisoned") = true;
        if let Some(listener) = self
            .listener
            .lock()
            .expect("cmi5 session lock poisoned")
            .take()
        {
            listener.stop();
        }
    }

    fn is_closed(&self) -> bool {
        *self.closed.lock().expect("cmi5 session lock poisoned")
    }
}

/// What the player asks to launch.
#[derive(Debug, Clone)]
pub struct LaunchRequest {
    pub attempt_id: String,
    /// The unit's identifier in the course structure.
    pub activity_id: String,
    /// The open package launch the unit runs under.
    pub prefix: String,
    /// The learner's languages, most preferred first, comma-separated, for
    /// the learner preferences document.
    pub languages: String,
}

/// `encodeURIComponent`: what the player uses for path segments, and what
/// launch parameter values are encoded with.
const COMPONENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'!')
    .remove(b'~')
    .remove(b'*')
    .remove(b'\'')
    .remove(b'(')
    .remove(b')');

fn encode(value: &str) -> String {
    utf8_percent_encode(value, COMPONENT).to_string()
}

fn random_hex(bytes: usize) -> String {
    let mut buffer = vec![0u8; bytes];
    rand::rngs::OsRng.fill_bytes(&mut buffer);
    hex::encode(buffer)
}

/// Appends `extra` to a URL's own `?query#fragment` (or makes the query),
/// before any fragment.
fn merge_query(own: Option<&str>, extra: &str) -> String {
    let own = own.unwrap_or("");
    let (query, fragment) = match own.split_once('#') {
        Some((query, fragment)) => (query, Some(fragment)),
        None => (own, None),
    };
    let query = query.strip_prefix('?').unwrap_or(query);
    let mut out = if query.is_empty() {
        format!("?{extra}")
    } else {
        format!("?{query}&{extra}")
    };
    if let Some(fragment) = fragment {
        out.push('#');
        out.push_str(fragment);
    }
    out
}

/// Whether a unit that asked for its own window gets one here: on desktop
/// yes; on mobile, which has one webview, it plays in the frame with focus
/// mode forced on (a declared deviation from cmi5 8.1).
const OWN_WINDOWS: bool = !cfg!(any(target_os = "ios", target_os = "android"));

/// Launches one unit of a cmi5 attempt: abandons any session of the attempt
/// still open, opens the session's endpoint, writes the launch data and
/// learner preferences, records `launched` (and `satisfied` for whatever the
/// moveOn criteria already satisfy) and returns the URL the player opens.
pub async fn launch<D>(
    db: Arc<D>,
    sessions: &Arc<Cmi5Sessions>,
    launches: &Launches,
    learner_key: &[u8; 32],
    request: LaunchRequest,
) -> Result<Cmi5Launch, AttemptError>
where
    D: PackageDb + Send + Sync + 'static,
{
    // cmi5 9.3.6: a launch in the same registration as an open session
    // abandons that session.
    for open in sessions.of_attempt(&request.attempt_id) {
        end(&*db, sessions, launches, &open).await?;
    }

    let attempt_id = request.attempt_id.clone();
    let unit_id = request.activity_id.clone();
    let key = *learner_key;
    let prepared = in_transaction(&*db, "packages.cmi5-prepare", move |db| {
        let conn = db.conn();
        let attempt = super::open_attempt(conn, &attempt_id)?;
        let package = super::stored_package(conn, &attempt.package_hash)?;
        if package.standard != Standard::Cmi5 {
            return Err(AttemptError::NotAPackage(format!(
                "package {} is not a cmi5 package",
                package.hash
            )));
        }
        let unit = super::launchable(&package, &unit_id)?;
        if unit.launch.as_ref().map(|l| l.kind) != Some(LaunchKind::AssignableUnit) {
            return Err(AttemptError::NotFound(format!(
                "activity {unit_id} is not an assignable unit"
            )));
        }
        // Sessions an earlier run of the app left open end here.
        store::abandon_stale(db, &attempt, &package)?;
        Ok((attempt, package))
    })
    .await?;
    let (attempt, package) = prepared;
    if launches.package_of(&request.prefix).as_deref() != Some(attempt.package_hash.as_str()) {
        return Err(AttemptError::NotFound(format!(
            "launch {} of package {}",
            request.prefix, attempt.package_hash
        )));
    }
    let unit = super::launchable(&package, &request.activity_id)?.clone();
    let launch = unit.launch.clone().expect("checked launchable");
    let cmi5 = unit.cmi5.clone().unwrap_or_default();
    let move_on = cmi5.move_on.unwrap_or(MoveOn::NotApplicable);
    let wants_own_window = cmi5.launch_method == Some(LaunchMethod::OwnWindow);

    let tcp = listener::bind(0).map_err(|e| AttemptError::Database(format!("endpoint: {e}")))?;
    let port = tcp
        .local_addr()
        .map_err(|e| AttemptError::Database(format!("endpoint: {e}")))?
        .port();

    let learner_id = progress::learner_id(learner_key, &package.hash);
    let session_id = uuid::Uuid::new_v4().to_string();
    let base_url = format!("{}/{}/", package_serving::origin(), request.prefix);
    let path: Vec<String> = launch.path.split('/').map(encode).collect();
    let unit_url = format!("{base_url}{}", path.join("/"));
    let facts_attempt = attempt.clone();
    let facts_unit = unit.id.clone();
    let facts = in_transaction(&*db, "packages.cmi5-facts", {
        let session_id = session_id.clone();
        let learner_id = learner_id.clone();
        move |db| {
            Ok(store::facts(
                db.conn(),
                &key,
                &facts_attempt,
                &facts_unit,
                &learner_id,
                &session_id,
                launch.mastery_score,
            ))
        }
    })
    .await?;

    let endpoint = format!("http://127.0.0.1:{port}/xapi/");
    let fetch_code = random_hex(16);
    let fetch = format!("http://127.0.0.1:{port}/fetch/{fetch_code}");
    let actor = serde_json::to_string(&facts.actor).expect("actor serialises");
    let parameters = format!(
        "endpoint={}&fetch={}&actor={}&registration={}&activityId={}",
        encode(&endpoint),
        encode(&fetch),
        encode(&actor),
        encode(&facts.registration),
        encode(&facts.activity_id),
    );
    let url = format!(
        "{unit_url}{}",
        merge_query(launch.parameters.as_deref(), &parameters)
    );
    let launch_url = format!("{unit_url}{}", launch.parameters.as_deref().unwrap_or(""));
    let return_url = format!("{base_url}{RETURN_PAGE}");

    let live = Arc::new(Live {
        id: session_id.clone(),
        attempt_id: attempt.id.clone(),
        unit_id: unit.id.clone(),
        package_hash: package.hash.clone(),
        prefix: request.prefix.clone(),
        facts: facts.clone(),
        token: random_hex(32),
        fetch_code,
        port,
        fetch_used: Mutex::new(false),
        record: Mutex::new(SessionRecord::default()),
        writing: tokio::sync::Mutex::new(()),
        closed: Mutex::new(false),
        listener: Mutex::new(None),
    });

    let begin = store::Begin {
        session_id: session_id.clone(),
        unit_id: unit.id.clone(),
        move_on,
        launch_parameters: launch.launch_data.clone(),
        entitlement_key: cmi5.entitlement_key.clone(),
        return_url,
        launch_url,
        languages: request.languages.clone(),
    };
    let begin_attempt = attempt.clone();
    let begin_package = package.clone();
    let begin_facts = facts.clone();
    let record = in_transaction(&*db, "packages.cmi5-launch", move |db| {
        store::begin(db, &begin_attempt, &begin_package, &begin_facts, &begin)
    })
    .await?;
    *live.record.lock().expect("cmi5 session lock poisoned") = record;

    *live.listener.lock().expect("cmi5 session lock poisoned") =
        Some(listener::serve(tcp, db.clone(), live.clone()));
    launches.set_endpoint_port(&request.prefix, Some(port));
    sessions.insert(live);

    Ok(Cmi5Launch {
        session_id,
        attempt_id: attempt.id,
        activity_id: unit.id,
        url,
        own_window: wants_own_window && OWN_WINDOWS,
        focus_forced: wants_own_window && !OWN_WINDOWS,
    })
}

/// Ends a session: its endpoint stops and the launch's policy no longer
/// admits it; a session the unit did not terminate is recorded `abandoned`.
/// Returns the attempt's progress. Ending a session that is not open (it
/// ended already, or the app restarted) still brings progress up to date.
pub async fn end<D: PackageDb>(
    db: &D,
    sessions: &Cmi5Sessions,
    launches: &Launches,
    session_id: &str,
) -> Result<PackageAttemptProgress, AttemptError> {
    let live = sessions.remove(session_id);
    if let Some(live) = &live {
        // Taken after any batch in flight is stored.
        let _writing = live.writing.lock().await;
        live.close();
        if !sessions
            .all()
            .iter()
            .any(|other| other.prefix == live.prefix)
        {
            launches.set_endpoint_port(&live.prefix, None);
        }
    }
    let session_id = session_id.to_string();
    let facts = live.as_ref().map(|live| live.facts.clone());
    in_transaction(db, "packages.cmi5-end", move |db| {
        store::finish(db, &session_id, facts.as_ref())
    })
    .await
}

/// Ends every open session of an attempt, as the player closes the package.
pub async fn end_attempt<D: PackageDb>(
    db: &D,
    sessions: &Cmi5Sessions,
    launches: &Launches,
    attempt_id: &str,
) -> Result<(), AttemptError> {
    for session in sessions.of_attempt(attempt_id) {
        end(db, sessions, launches, &session).await?;
    }
    Ok(())
}

/// The launch mode every unit gets: the app has no browse or review offer.
const LAUNCH_MODE: LaunchMode = LaunchMode::Normal;

fn millis(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// A unit's moveOn criteria, as its description says.
fn move_on_of(unit: &alexandria_studio::package::Activity) -> MoveOn {
    unit.cmi5
        .as_ref()
        .and_then(|cmi5| cmi5.move_on)
        .unwrap_or(MoveOn::NotApplicable)
}
