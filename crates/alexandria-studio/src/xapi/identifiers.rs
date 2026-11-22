//! Activity identifiers and per-recipient identifiers.

use std::fmt;

use alexandria_verify::did::{derive_did_key, parse_did_key, Did};
use ed25519_dalek::SigningKey;
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::statement::Account;
use super::{StatementError, ACCOUNT_HOME_PAGE, MAX_IRI_LEN};

/// Everything but the RFC 3986 unreserved characters is percent-encoded,
/// with upper-case hex. Stores compare identifiers byte for byte, so this is
/// the only form ever emitted.
const SEGMENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

const RECIPIENT_KEY_INFO: &str = "alexandria/xapi/recipient/v1|";
const ID_KEY_INFO: &str = "alexandria/xapi/ids/v1|";

/// An activity in a course, named by the DID that created the course.
///
/// The identifier never changes with the course's owner, staff or
/// visibility, because `creator_did` is the DID fixed in the course's
/// identity. Nesting is course → element → package activity → interaction;
/// build with [`ActivityRef::course`] and the chained methods.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActivityRef {
    pub creator_did: String,
    pub course_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub element_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package_activity_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interaction_id: Option<String>,
}

/// How deep an [`ActivityRef`] goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Course,
    Element,
    PackageActivity,
    Interaction,
}

impl ActivityRef {
    pub fn course(creator_did: impl Into<String>, course_id: impl Into<String>) -> Self {
        Self {
            creator_did: creator_did.into(),
            course_id: course_id.into(),
            element_id: None,
            package_hash: None,
            package_activity_id: None,
            interaction_id: None,
        }
    }

    pub fn element(&self, element_id: impl Into<String>) -> Self {
        Self {
            element_id: Some(element_id.into()),
            ..self.clone()
        }
    }

    /// An activity inside the package element, by the package's content hash
    /// and the package's own identifier for the activity.
    pub fn package_activity(
        &self,
        package_hash: impl Into<String>,
        activity_id: impl Into<String>,
    ) -> Self {
        Self {
            package_hash: Some(package_hash.into()),
            package_activity_id: Some(activity_id.into()),
            ..self.clone()
        }
    }

    pub fn interaction(&self, interaction_id: impl Into<String>) -> Self {
        Self {
            interaction_id: Some(interaction_id.into()),
            ..self.clone()
        }
    }

    /// The nesting level, refusing a reference with a gap in it.
    pub fn level(&self) -> Result<Level, StatementError> {
        let nesting = (
            self.element_id.is_some(),
            self.package_hash.is_some(),
            self.package_activity_id.is_some(),
            self.interaction_id.is_some(),
        );
        match nesting {
            (false, false, false, false) => Ok(Level::Course),
            (true, false, false, false) => Ok(Level::Element),
            (true, true, true, false) => Ok(Level::PackageActivity),
            (true, true, true, true) => Ok(Level::Interaction),
            _ => Err(StatementError::InvalidActivity(
                "an activity reference must nest course, element, package activity, interaction"
                    .into(),
            )),
        }
    }

    /// The activity that contains this one, if any.
    pub fn parent(&self) -> Option<Self> {
        let mut parent = self.clone();
        if parent.interaction_id.take().is_some() {
            return Some(parent);
        }
        if parent.package_activity_id.take().is_some() {
            parent.package_hash = None;
            return Some(parent);
        }
        parent.element_id.take().map(|_| parent)
    }

    /// The course this activity belongs to.
    pub fn course_ref(&self) -> Self {
        Self::course(self.creator_did.clone(), self.course_id.clone())
    }

    /// The activity's IRI.
    ///
    /// ```text
    /// course       = <creator DID> "/courses/" <course id>
    /// element      = course "/elements/" <element id>
    /// pkg activity = element "/packages/" <package hash> "/activities/" seg(<id>)
    /// interaction  = pkg activity "/interactions/" seg(<id>)
    /// ```
    ///
    /// An identifier longer than [`MAX_IRI_LEN`] falls back to
    /// `"/activity-digests/"` or `"/interaction-digests/"` followed by the
    /// SHA-256 of the raw identifier. Those words cannot come out of `seg`, so
    /// the forms never collide.
    pub fn iri(&self) -> Result<String, StatementError> {
        let level = self.level()?;
        parse_did_key(&self.creator_did)
            .map_err(|e| StatementError::InvalidDid(format!("course creator: {e}")))?;
        let course = format!(
            "{}/courses/{}",
            self.creator_did,
            entity_id("course id", &self.course_id)?
        );
        if level == Level::Course {
            return Ok(course);
        }
        let element = format!(
            "{course}/elements/{}",
            entity_id("element id", self.element_id.as_deref().unwrap_or_default())?
        );
        if level == Level::Element {
            return Ok(element);
        }
        let package = format!(
            "{element}/packages/{}",
            entity_id(
                "package hash",
                self.package_hash.as_deref().unwrap_or_default()
            )?
        );
        let raw_activity = self.package_activity_id.as_deref().unwrap_or_default();
        let digest_activity = format!("{package}/activity-digests/{}", digest(raw_activity));
        let activity = capped(
            format!("{package}/activities/{}", segment(raw_activity)?),
            &digest_activity,
        );
        if level == Level::PackageActivity {
            return Ok(activity);
        }
        let raw_interaction = self.interaction_id.as_deref().unwrap_or_default();
        let full = format!("{activity}/interactions/{}", segment(raw_interaction)?);
        if full.len() <= MAX_IRI_LEN {
            return Ok(full);
        }
        let digested = format!("{activity}/interaction-digests/{}", digest(raw_interaction));
        if digested.len() <= MAX_IRI_LEN {
            return Ok(digested);
        }
        // The package activity itself sits just under the cap: hang the
        // interaction digest off the activity's digest form instead.
        Ok(format!(
            "{digest_activity}/interaction-digests/{}",
            digest(raw_interaction)
        ))
    }
}

fn capped(full: String, fallback: &str) -> String {
    if full.len() <= MAX_IRI_LEN {
        full
    } else {
        fallback.to_string()
    }
}

/// A 64-character lower-case hex entity id or content hash.
fn entity_id<'a>(what: &str, id: &'a str) -> Result<&'a str, StatementError> {
    let ok = id.len() == 64 && id.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
    if ok {
        Ok(id)
    } else {
        Err(StatementError::InvalidActivity(format!(
            "{what} must be 64 lower-case hex characters"
        )))
    }
}

/// One path segment in the canonical encoding. A bare `.` or `..` would be a
/// dot segment, so it is encoded too.
fn segment(raw: &str) -> Result<String, StatementError> {
    match raw {
        "" => Err(StatementError::InvalidActivity(
            "a package activity or interaction id is empty".into(),
        )),
        "." => Ok("%2E".into()),
        ".." => Ok("%2E%2E".into()),
        _ => Ok(utf8_percent_encode(raw, SEGMENT).to_string()),
    }
}

fn digest(raw: &str) -> String {
    hex(&Sha256::digest(raw.as_bytes()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn hkdf32(learner_secret: &[u8; 32], info: &str) -> [u8; 32] {
    let mut out = [0u8; 32];
    Hkdf::<Sha256>::new(None, learner_secret)
        .expand(info.as_bytes(), &mut out)
        .expect("32 bytes is a valid HKDF-SHA256 output length");
    out
}

fn recipient_label(label: &str) -> Result<&str, StatementError> {
    if label.is_empty() {
        Err(StatementError::InvalidIdentifier(
            "the recipient label is empty".into(),
        ))
    } else {
        Ok(label)
    }
}

/// The recipient's own `did:key`: an Ed25519 key whose seed is
/// HKDF-SHA256(learner key material, info `alexandria/xapi/recipient/v1|<label>`).
/// It cannot be linked to the learner's DID without the learner's key.
pub fn pairwise_did(learner_secret: &[u8; 32], label: &str) -> Result<Did, StatementError> {
    let label = recipient_label(label)?;
    let seed = hkdf32(learner_secret, &format!("{RECIPIENT_KEY_INFO}{label}"));
    Ok(derive_did_key(&SigningKey::from_bytes(&seed)))
}

/// The key statement and registration identifiers are derived from for one
/// recipient: HKDF-SHA256(learner key material, empty salt,
/// info `alexandria/xapi/ids/v1|<label>`).
#[derive(Clone)]
pub struct IdKey([u8; 32]);

impl IdKey {
    pub fn derive(learner_secret: &[u8; 32], label: &str) -> Result<Self, StatementError> {
        let label = recipient_label(label)?;
        Ok(Self(hkdf32(
            learner_secret,
            &format!("{ID_KEY_INFO}{label}"),
        )))
    }

    /// UUIDv8 from HMAC-SHA256(key, `<kind>|<local id>`), first 16 bytes,
    /// with the version and variant bits set (stores reject a raw hash).
    fn uuid(&self, kind: &str, local_id: &str) -> String {
        let mut mac =
            Hmac::<Sha256>::new_from_slice(&self.0).expect("HMAC-SHA256 takes a key of any size");
        mac.update(kind.as_bytes());
        mac.update(b"|");
        mac.update(local_id.as_bytes());
        let tag = mac.finalize().into_bytes();
        let mut b = [0u8; 16];
        b.copy_from_slice(&tag[..16]);
        b[6] = (b[6] & 0x0f) | 0x80;
        b[8] = (b[8] & 0x3f) | 0x80;
        uuid::Uuid::from_bytes(b).hyphenated().to_string()
    }
}

impl fmt::Debug for IdKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("IdKey(..)")
    }
}

/// Who a statement is built for: the actor it names and the key its
/// statement and registration identifiers come from.
///
/// The learner's own log is a recipient too: [`Recipient::real_did`] with a
/// label of the caller's choosing names the learner by DID.
#[derive(Debug, Clone)]
pub struct Recipient {
    account: Account,
    id_key: IdKey,
}

impl Recipient {
    pub fn new(account: Account, id_key: IdKey) -> Self {
        Self { account, id_key }
    }

    /// The default: an actor from the recipient's [`pairwise_did`].
    pub fn pairwise(learner_secret: &[u8; 32], label: &str) -> Result<Self, StatementError> {
        let did = pairwise_did(learner_secret, label)?;
        Ok(Self::new(
            Account::for_did(did.as_str())?,
            IdKey::derive(learner_secret, label)?,
        ))
    }

    /// The learner chose to be named by their own DID. Statement and
    /// registration identifiers are still per recipient.
    pub fn real_did(
        learner_secret: &[u8; 32],
        learner_did: &str,
        label: &str,
    ) -> Result<Self, StatementError> {
        Ok(Self::new(
            Account::for_did(learner_did)?,
            IdKey::derive(learner_secret, label)?,
        ))
    }

    /// The learner approved an account the recipient proposed, used verbatim.
    pub fn proposed(
        learner_secret: &[u8; 32],
        account: Account,
        label: &str,
    ) -> Result<Self, StatementError> {
        account.validate()?;
        Ok(Self::new(account, IdKey::derive(learner_secret, label)?))
    }

    pub fn account(&self) -> &Account {
        &self.account
    }

    /// The recipient's identifier for a local statement.
    pub fn statement_id(&self, local_id: &str) -> String {
        self.id_key.uuid("statement", local_id)
    }

    /// The recipient's identifier for a local registration (an enrolment or
    /// a package attempt).
    pub fn registration(&self, local_registration: &str) -> String {
        self.id_key.uuid("registration", local_registration)
    }
}

impl Account {
    /// The account for a `did:key`: [`ACCOUNT_HOME_PAGE`] and the DID without
    /// its `did:key:` prefix (48 characters for Ed25519).
    pub fn for_did(did: &str) -> Result<Self, StatementError> {
        parse_did_key(did).map_err(|e| StatementError::InvalidDid(e.to_string()))?;
        let name = did.strip_prefix("did:key:").unwrap_or(did);
        let account = Self {
            home_page: ACCOUNT_HOME_PAGE.to_string(),
            name: name.to_string(),
        };
        account.validate()?;
        Ok(account)
    }
}
