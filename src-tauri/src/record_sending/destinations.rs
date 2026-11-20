//! Record destinations: stores the learner names, contacted directly with
//! the credentials they issued to the learner. Core: nothing here needs the
//! hosted service or the organisations switch.
//!
//! - [`preview`] looks the address up and shows the identifiers records
//!   would carry. It sends nothing.
//! - [`add`] is the learner's approval: a private or on-device address
//!   needs `confirm_private`, the version is negotiated with the store, and
//!   records in scope are queued.
//! - [`send_pending`] drains every destination's outbox; [`run`] calls it in
//!   the background while the profile is open, and [`wake`] asks for a pass
//!   now.
//! - [`remove`] stops sending at once, then asks the store to void what it
//!   was sent. Records already sent stay with the store's operator.
//!
//! The store's secret is a row in the profile's encrypted database, like
//! other secrets the app keeps for a learner, and never leaves this module
//! except in the request's authorisation header.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use alexandria_ai::endpoint::Location;
use alexandria_studio::xapi::lrs::{self, Credentials, Lrs, LrsError, StoreAddress};
use alexandria_studio::xapi::{Account, Statement};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use tokio::sync::Notify;

use super::{
    access_log, outbox, recipients, send_due, timestamp, void_sent, BatchOutcome, Identity,
    RecipientEntry, RecipientKind, RecordDb, Scope, SendError, Transport,
};
use crate::domain::record_sending::{
    CourseChoice, DestinationPreview, IdentifierChoice, NewDestinationRequest, RecordDestination,
    RemovalReport,
};
use crate::p2p::inbound::InboundDatabase;

/// The recipient label of a destination: the store's address and the key it
/// issued. Never change the format: it keys the actor the store sees, and
/// with the approval's id ([`recipients::instance_label`]) its statement and
/// registration identifiers.
pub fn label(endpoint: &str, username: &str) -> String {
    format!("destination|{endpoint}|{username}")
}

/// A destination with what is needed to reach it.
#[derive(Debug, Clone)]
pub struct Destination {
    pub recipient: RecipientEntry,
    pub endpoint: String,
    pub location: Location,
    pub username: String,
    pub xapi_version: String,
}

fn check_credentials(username: &str, secret: Option<&str>) -> Result<(), SendError> {
    if username.trim().is_empty() {
        return Err(SendError::InvalidCredentials(
            "enter the key the store issued to you".into(),
        ));
    }
    if username.contains(':') || username.chars().any(char::is_control) {
        return Err(SendError::InvalidCredentials(
            "a store's key cannot contain a colon".into(),
        ));
    }
    if secret.is_some_and(|secret| secret.is_empty()) {
        return Err(SendError::InvalidCredentials(
            "enter the secret the store issued to you".into(),
        ));
    }
    Ok(())
}

fn actor_name(
    identity: &Identity,
    secret: &[u8; 32],
    did: &str,
    label: &str,
) -> Result<String, SendError> {
    Ok(recipients::recipient(identity, secret, did, label)?
        .account()
        .name
        .clone())
}

fn enrolled_courses(conn: &Connection) -> Result<Vec<CourseChoice>, SendError> {
    let mut statement = conn.prepare(
        "SELECT DISTINCT c.id, c.title FROM enrollments e JOIN courses c ON c.id = e.course_id \
         ORDER BY c.title, c.id",
    )?;
    let rows = statement.query_map([], |row| {
        Ok(CourseChoice {
            id: row.get(0)?,
            title: row.get(1)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// What the consent screen shows for a store before the learner approves
/// it. Looks the address up; sends nothing to it.
pub async fn preview<D: RecordDb>(
    db: &D,
    address: &str,
    username: &str,
    learner_secret: &[u8; 32],
    learner_did: &str,
) -> Result<DestinationPreview, String> {
    check_credentials(username, None).map_err(|e| e.to_string())?;
    let store = lrs::resolve(address).await.map_err(|e| e.to_string())?;
    let endpoint = store.base().to_string();
    let label = label(&endpoint, username.trim());
    let pairwise_actor = actor_name(&Identity::Pairwise, learner_secret, learner_did, &label)
        .map_err(|e| e.to_string())?;
    let real_did_actor = Account::for_did(learner_did)
        .map_err(|e| e.to_string())?
        .name;
    let courses = db
        .run("records.destinations.courses", |db| {
            enrolled_courses(db.conn()).map_err(|e| e.to_string())
        })
        .await?;
    Ok(DestinationPreview {
        endpoint,
        location: store.location().as_str().to_string(),
        needs_confirmation: store.location() != Location::Remote,
        pairwise_actor,
        real_did_actor,
        courses,
    })
}

/// The learner approved a store: checks the address and the confirmation a
/// private or on-device one needs, negotiates the version, stores the
/// destination and its credentials, and queues the records in scope.
pub async fn add<D: RecordDb>(
    db: &D,
    request: NewDestinationRequest,
    learner_secret: &[u8; 32],
    learner_did: &str,
    now: DateTime<Utc>,
) -> Result<RecordDestination, String> {
    let username = request.username.trim().to_string();
    check_credentials(&username, Some(&request.secret)).map_err(|e| e.to_string())?;
    let identity = match request.identifier {
        IdentifierChoice::Pairwise => Identity::Pairwise,
        IdentifierChoice::RealDid => Identity::RealDid,
        // Only a record share request carries a proposed account.
        IdentifierChoice::Proposed => {
            return Err(SendError::InvalidCredentials(
                "a store you add yourself has no proposed account".into(),
            )
            .to_string())
        }
    };
    let scope = Scope {
        course_ids: request.course_ids,
        include_interactions: request.include_interactions,
    };
    scope.validate().map_err(|e| e.to_string())?;
    let store = lrs::resolve(&request.address)
        .await
        .map_err(|e| e.to_string())?;
    if store.location() != Location::Remote && !request.confirm_private {
        return Err(SendError::ConfirmationRequired(
            "this store is on this device or a private network; confirm that you want to send records there"
                .into(),
        )
        .to_string());
    }
    let version = lrs::negotiate(&store).await.map_err(|e| e.to_string())?;

    let endpoint = store.base().to_string();
    let label = label(&endpoint, &username);
    let entry = RecipientEntry {
        id: uuid::Uuid::new_v4().to_string(),
        kind: RecipientKind::Destination,
        actor_name: actor_name(&identity, learner_secret, learner_did, &label)
            .map_err(|e| e.to_string())?,
        label,
        identity,
        scope,
        created_at: timestamp(now),
    };
    let destination = Destination {
        recipient: entry,
        endpoint,
        location: store.location(),
        username,
        xapi_version: version.to_string(),
    };
    let secret = request.secret;
    let view = db
        .run("records.destinations.add", move |db| {
            let conn = db.conn();
            crate::db::with_typed_transaction(conn, || {
                insert(conn, &destination, &secret)?;
                outbox::queue(conn, &destination.recipient, now)?;
                view(conn, &destination)
            })
            .map_err(|e: SendError| e.to_string())
        })
        .await?;
    wake();
    Ok(view)
}

fn insert(conn: &Connection, destination: &Destination, secret: &str) -> Result<(), SendError> {
    for course_id in &destination.recipient.scope.course_ids {
        let known: bool = conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM courses WHERE id = ?1)",
            params![course_id],
            |row| row.get(0),
        )?;
        if !known {
            return Err(SendError::InvalidScope(format!(
                "course {course_id} is not on this device"
            )));
        }
    }
    recipients::insert(conn, &destination.recipient)?;
    conn.execute(
        "INSERT INTO record_destinations (recipient_id, endpoint, location, username, xapi_version) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            destination.recipient.id,
            destination.endpoint,
            destination.location.as_str(),
            destination.username,
            destination.xapi_version,
        ],
    )?;
    conn.execute(
        "INSERT INTO record_destination_secrets (recipient_id, secret) VALUES (?1, ?2)",
        params![destination.recipient.id, secret],
    )?;
    Ok(())
}

fn view(conn: &Connection, destination: &Destination) -> Result<RecordDestination, SendError> {
    Ok(RecordDestination {
        id: destination.recipient.id.clone(),
        endpoint: destination.endpoint.clone(),
        location: destination.location.as_str().to_string(),
        username: destination.username.clone(),
        identifier: destination.recipient.identity.as_str().to_string(),
        actor_name: destination.recipient.actor_name.clone(),
        scope: destination.recipient.scope.clone(),
        xapi_version: destination.xapi_version.clone(),
        created_at: destination.recipient.created_at.clone(),
        status: outbox::status(conn, &destination.recipient.id)?,
    })
}

/// One destination and its store-issued secret.
fn load(
    conn: &Connection,
    recipient: RecipientEntry,
) -> Result<Option<(Destination, String)>, SendError> {
    let row = conn
        .query_row(
            "SELECT d.endpoint, d.location, d.username, d.xapi_version, s.secret \
             FROM record_destinations d JOIN record_destination_secrets s USING (recipient_id) \
             WHERE d.recipient_id = ?1",
            params![recipient.id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            },
        )
        .optional()?;
    let Some((endpoint, location, username, xapi_version, secret)) = row else {
        return Ok(None);
    };
    let location = Location::from_legacy(&location)
        .ok_or_else(|| SendError::Database(format!("unknown location {location}")))?;
    Ok(Some((
        Destination {
            recipient,
            endpoint,
            location,
            username,
            xapi_version,
        },
        secret,
    )))
}

fn load_all(conn: &Connection) -> Result<Vec<(Destination, String)>, SendError> {
    let mut destinations = Vec::new();
    for recipient in recipients::list(conn, RecipientKind::Destination)? {
        if let Some(found) = load(conn, recipient)? {
            destinations.push(found);
        }
    }
    Ok(destinations)
}

/// Every destination with its outbox status, oldest first.
pub fn list(conn: &Connection) -> Result<Vec<RecordDestination>, SendError> {
    load_all(conn)?
        .iter()
        .map(|(destination, _)| view(conn, destination))
        .collect()
}

/// Posts statements to a destination's store.
struct StoreTransport(Result<Lrs, LrsError>);

impl Transport for StoreTransport {
    fn deliver<'a>(
        &'a self,
        statements: &'a [Statement],
    ) -> Pin<Box<dyn Future<Output = Result<(), LrsError>> + Send + 'a>> {
        Box::pin(async move {
            match &self.0 {
                Ok(store) => store.post_statements(statements).await,
                Err(error) => Err(error.clone()),
            }
        })
    }
}

/// Looks the store up again before each pass. An address that now resolves
/// to a different kind of network than the learner approved is not used.
async fn connect(destination: &Destination, secret: String) -> StoreTransport {
    let store: Result<StoreAddress, LrsError> = lrs::resolve(&destination.endpoint).await;
    StoreTransport(store.and_then(|store| {
        if store.location() != destination.location {
            return Err(LrsError::InvalidAddress(
                "the store's address now leads to a different network than the one you approved; remove it and add it again"
                    .into(),
            ));
        }
        Lrs::new(
            store,
            Credentials {
                username: destination.username.clone(),
                secret,
            },
            destination.xapi_version.clone(),
        )
    }))
}

/// Sends what is due to every destination.
pub async fn send_pending<D: RecordDb>(
    db: &D,
    learner_secret: &[u8; 32],
    learner_did: &str,
    now: DateTime<Utc>,
) -> Result<BatchOutcome, String> {
    let destinations = db
        .run("records.destinations.load", |db| {
            load_all(db.conn()).map_err(|e| e.to_string())
        })
        .await?;
    let mut total = BatchOutcome::default();
    for (destination, secret) in destinations {
        let transport = connect(&destination, secret).await;
        match send_due(
            db,
            &destination.recipient,
            &destination.endpoint,
            &transport,
            learner_secret,
            learner_did,
            now,
        )
        .await
        {
            Ok(outcome) => {
                total.sent += outcome.sent;
                total.failed += outcome.failed;
                total.waiting += outcome.waiting;
            }
            Err(error) => log::warn!("sending to a record destination failed: {error}"),
        }
    }
    Ok(total)
}

/// Removes a destination. Sending stops before anything else happens; the
/// store is then asked, once, to void every record it accepted. Records
/// already sent stay with the store's operator whatever it does with that.
pub async fn remove<D: RecordDb>(
    db: &D,
    id: &str,
    learner_secret: &[u8; 32],
    learner_did: &str,
    now: DateTime<Utc>,
) -> Result<RemovalReport, String> {
    let id = id.to_string();
    let removed = db
        .run("records.destinations.remove", move |db| {
            let conn = db.conn();
            crate::db::with_typed_transaction(conn, || {
                let Some(recipient) = recipients::get(conn, &id)? else {
                    return Ok(None);
                };
                if recipient.kind != RecipientKind::Destination {
                    return Ok(None);
                }
                let found = load(conn, recipient)?;
                let accepted = access_log::accepted_record_ids(conn, &id)?;
                recipients::delete(conn, &id)?;
                Ok(found.map(|found| (found, accepted)))
            })
            .map_err(|e: SendError| e.to_string())
        })
        .await?;
    let Some(((destination, secret), accepted)) = removed else {
        return Err(SendError::NotFound("no such record destination".into()).to_string());
    };
    if accepted.is_empty() {
        return Ok(RemovalReport {
            voided: 0,
            void_error: None,
        });
    }
    let transport = connect(&destination, secret).await;
    let report = match void_sent(
        db,
        &destination.recipient,
        &destination.endpoint,
        &transport,
        learner_secret,
        learner_did,
        accepted,
        now,
    )
    .await
    {
        Ok(voided) => RemovalReport {
            voided: voided as u32,
            void_error: None,
        },
        Err(error) => RemovalReport {
            voided: 0,
            void_error: Some(error),
        },
    };
    Ok(report)
}

static WAKE: LazyLock<Notify> = LazyLock::new(Notify::new);

/// Asks the background sender for a pass now.
pub fn wake() {
    WAKE.notify_one();
}

/// The longest the background sender sleeps between passes. New records
/// are picked up on the next pass.
const IDLE: Duration = Duration::from_secs(60);

/// The background sender, a profile job: a pass whenever woken, when a
/// retry falls due, and at least every [`IDLE`]. Each pass serves record
/// destinations and then record shares ([`super::shares::send_pending`]).
pub(crate) async fn run(
    db: InboundDatabase,
    keystore: Arc<tokio::sync::Mutex<Option<crate::crypto::keystore::Keystore>>>,
) {
    loop {
        let wait = pass(&db, &keystore).await;
        tokio::select! {
            _ = tokio::time::sleep(wait) => {}
            _ = WAKE.notified() => {}
        }
    }
}

async fn pass(
    db: &InboundDatabase,
    keystore: &tokio::sync::Mutex<Option<crate::crypto::keystore::Keystore>>,
) -> Duration {
    let any = db
        .run("records.recipients.any", |db| {
            db.conn()
                .query_row(
                    "SELECT EXISTS (SELECT 1 FROM record_recipients)",
                    [],
                    |row| row.get::<_, bool>(0),
                )
                .map_err(|e| e.to_string())
        })
        .await;
    match any {
        Ok(true) => {}
        Ok(false) => return IDLE,
        Err(error) => {
            log::debug!("record destinations not checked: {error}");
            return IDLE;
        }
    }
    let issuer = match crate::credentials::issuer::Issuer::from_keystore(keystore).await {
        Ok(issuer) => issuer,
        Err(_) => return IDLE,
    };
    let secret = zeroize::Zeroizing::new(issuer.key().to_bytes());
    let did = issuer.did().as_str().to_string();
    if let Err(error) = send_pending(db, &secret, &did, Utc::now()).await {
        log::warn!("record destinations pass failed: {error}");
    }
    if let Err(error) = super::shares::send_pending(db, issuer.key(), Utc::now()).await {
        log::warn!("record shares pass failed: {error}");
    }
    let next = db
        .run("records.outbox.next-retry", |db| {
            outbox::next_retry(db.conn()).map_err(|e| e.to_string())
        })
        .await
        .ok()
        .flatten();
    match next {
        Some(at) => (at - Utc::now())
            .to_std()
            .unwrap_or(Duration::from_secs(1))
            .clamp(Duration::from_secs(1), IDLE),
        None => IDLE,
    }
}
