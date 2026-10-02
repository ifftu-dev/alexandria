use alexandria_verify::qualification::QualificationPolicySet;
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use rusqlite::{params, Connection, OptionalExtension};

use crate::crypto::{did::did_from_verifying_key, hash::entity_id};
use crate::domain::discussions::*;

use super::opinion_eligibility::{
    evaluate_opinion_credential, OpinionCredentialEligibility, MAX_OPINION_CREDENTIAL_PROOFS,
};

pub const MAX_EVENT_BYTES: usize = 64 * 1024;

pub fn sign(payload: DiscussionPayload, key: &SigningKey) -> Result<DiscussionEvent, String> {
    let bytes = serde_json_canonicalizer::to_vec(&payload).map_err(|e| e.to_string())?;
    Ok(DiscussionEvent {
        discussion_version: 1,
        payload,
        public_key: hex::encode(key.verifying_key().to_bytes()),
        signature: hex::encode(key.sign(&bytes).to_bytes()),
    })
}

fn verify(event: &DiscussionEvent, network: &str) -> Result<String, String> {
    if event.discussion_version != 1
        || event.payload.version != 1
        || event.payload.network_id != network
    {
        return Err("unsupported discussion version or network".into());
    }
    let bytes = serde_json_canonicalizer::to_vec(&event.payload).map_err(|e| e.to_string())?;
    if bytes.len() > MAX_EVENT_BYTES {
        return Err("discussion event too large".into());
    }
    let key_bytes: [u8; 32] = hex::decode(&event.public_key)
        .map_err(|e| e.to_string())?
        .try_into()
        .map_err(|_| "invalid public key")?;
    let key = VerifyingKey::from_bytes(&key_bytes).map_err(|e| e.to_string())?;
    if did_from_verifying_key(&key).as_str() != event.payload.actor_did {
        return Err("discussion actor does not match signing key".into());
    }
    let sig = Signature::from_slice(&hex::decode(&event.signature).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    key.verify_strict(&bytes, &sig)
        .map_err(|_| "invalid discussion signature")?;
    Ok(blake3::hash(&bytes).to_hex().to_string())
}

fn validate_content(c: &PostContent) -> Result<(), String> {
    if c.title.trim().is_empty() || c.title.chars().count() > 300 || c.body.chars().count() > 20_000
    {
        return Err(
            "use a title of 1–300 characters and a body of at most 20,000 characters".into(),
        );
    }
    match c.post_kind.as_str() {
        "text" if !c.body.trim().is_empty() && c.url.is_none() && c.video_cid.is_none() => {}
        "link" if c.video_cid.is_none() => {
            let u =
                url::Url::parse(c.url.as_deref().unwrap_or("")).map_err(|_| "invalid link URL")?;
            if !matches!(u.scheme(), "https" | "http")
                || u.host_str().is_none()
                || !u.username().is_empty()
                || u.password().is_some()
            {
                return Err("link must be a public HTTP(S) URL without credentials".into());
            }
        }
        "video" if c.url.is_none() && c.video_cid.as_deref().is_some_and(valid_cid) => {}
        _ => return Err("choose text, link, or video and provide its content".into()),
    }
    if c.thumbnail_cid.as_deref().is_some_and(|c| !valid_cid(c)) {
        return Err("invalid thumbnail CID".into());
    }
    Ok(())
}
fn valid_cid(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|c| c.is_ascii_hexdigit())
}

/// Both local actions and network deliveries pass this validator. Pending
/// dependencies stay invisible until independently verified on this peer.
pub fn ingest(
    conn: &Connection,
    event: &DiscussionEvent,
    policies: &QualificationPolicySet,
    network: &str,
    now: &str,
) -> Result<bool, String> {
    let id = verify(event, network)?;
    let p = &event.payload;
    if p.created_at < 0
        || p.created_at > chrono::Utc::now().timestamp() + 300
        || p.revision < 0
        || p.nonce.len() > 64
        || p.credential_proof_ids.len() > MAX_OPINION_CREDENTIAL_PROOFS
        || p.entity_id.len() > 128
        || p.thread_id.len() > 128
        || p.subject_field_id.len() > 128
    {
        return Err("invalid discussion bounds".into());
    }
    let exists: Option<i64> = conn
        .query_row(
            "SELECT accepted FROM discussion_events WHERE id=?1",
            [&id],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    if exists == Some(1) {
        return Ok(true);
    }
    match &p.action {
        DiscussionAction::Post { content } | DiscussionAction::EditPost { content } => {
            validate_content(content)?
        }
        DiscussionAction::Comment { body } | DiscussionAction::EditComment { body }
            if body.trim().is_empty() || body.chars().count() > 10_000 =>
        {
            return Err("comment must contain 1–10,000 characters".into())
        }
        DiscussionAction::Vote { value } if !(-1..=1).contains(value) => {
            return Err("invalid vote".into())
        }
        DiscussionAction::Report { reason }
            if !["spam", "harassment", "misinformation", "off_topic", "other"]
                .contains(&reason.as_str()) =>
        {
            return Err("invalid report reason".into())
        }
        _ => {}
    }
    let creating = matches!(
        p.action,
        DiscussionAction::Post { .. } | DiscussionAction::Comment { .. }
    );
    if creating && (p.entity_id != creation_id(p)? || p.revision != 0 || p.nonce.is_empty()) {
        return Err("invalid new discussion identity".into());
    }
    if matches!(p.action, DiscussionAction::Post { .. })
        && (p.thread_id != p.entity_id || p.parent_id.is_some())
    {
        return Err("invalid thread root".into());
    }
    let mut pending = false;
    let target = load_item(conn, &p.entity_id, &p.actor_did)?;
    if !matches!(p.action, DiscussionAction::Post { .. }) {
        let root = load_item(conn, &p.thread_id, &p.actor_did)?;
        if let Some(root) = root {
            if root.parent_id.is_some() || root.subject_field_id != p.subject_field_id {
                return Err("thread topic mismatch".into());
            }
        } else {
            pending = true;
        }
        if matches!(p.action, DiscussionAction::Comment { .. }) {
            let parent_id = p.parent_id.as_deref().ok_or("comment needs a parent")?;
            if parent_id == p.entity_id {
                return Err("comment cannot parent itself".into());
            }
            let mut parent = load_item(conn, parent_id, &p.actor_did)?;
            let mut depth = 0;
            while let Some(item) = parent {
                if item.thread_id != p.thread_id {
                    return Err("comment parent belongs to another thread".into());
                }
                depth += 1;
                if depth > 8 {
                    return Err("maximum comment depth is 8".into());
                }
                parent = match item.parent_id {
                    Some(ref parent) => load_item(conn, parent, &p.actor_did)?,
                    None => break,
                };
            }
            if depth == 0 {
                pending = true;
            }
        } else if let Some(target) = &target {
            if target.thread_id != p.thread_id || target.subject_field_id != p.subject_field_id {
                return Err("target topic mismatch".into());
            }
            if matches!(
                p.action,
                DiscussionAction::EditPost { .. }
                    | DiscussionAction::EditComment { .. }
                    | DiscussionAction::Delete
            ) && target.author_did != p.actor_did
            {
                return Err("only the author may edit or delete".into());
            }
            if matches!(p.action, DiscussionAction::EditPost { .. }) && target.parent_id.is_some()
                || matches!(p.action, DiscussionAction::EditComment { .. })
                    && target.parent_id.is_none()
            {
                return Err("edit type mismatch".into());
            }
        } else {
            pending = true;
        }
    }
    if creating
        || matches!(
            p.action,
            DiscussionAction::EditPost { .. } | DiscussionAction::EditComment { .. }
        )
    {
        let mut qualified = false;
        let mut unresolved = false;
        let actor = serde_json::from_value(serde_json::Value::String(p.actor_did.clone()))
            .map_err(|e| e.to_string())?;
        if p.qualification_proofs.len() != p.credential_proof_ids.len()
            || p.qualification_proofs.len() > MAX_OPINION_CREDENTIAL_PROOFS
        {
            return Err("qualification proof count mismatch".into());
        }
        for (proof, id) in p.qualification_proofs.iter().zip(&p.credential_proof_ids) {
            if proof.credential.id.as_ref() != Some(id) {
                return Err("qualification credential ID mismatch".into());
            }
            match evaluate_opinion_credential(
                conn,
                policies,
                &proof.credential,
                &actor,
                &p.subject_field_id,
                now,
                proof
                    .endorsement
                    .as_ref()
                    .map(DiscussionEndorsement::as_evidence),
            )? {
                OpinionCredentialEligibility::Qualified(_) => {
                    qualified = true;
                    break;
                }
                OpinionCredentialEligibility::Pending(_)
                | OpinionCredentialEligibility::Unknown => unresolved = true,
                OpinionCredentialEligibility::Unqualified(_) => {}
            }
        }
        if !qualified {
            if !unresolved
                || policies
                    .applicable(
                        alexandria_verify::qualification::QualificationAction::OpinionPosting,
                        &p.subject_field_id,
                    )
                    .is_none()
            {
                return Err("posting and commenting require a credential accepted by this topic's pinned qualification policy".into());
            }
            pending = true;
        }
    }
    let signed = serde_json::to_string(event).map_err(|e| e.to_string())?;
    if pending {
        let count: i64 = conn
            .query_row(
                "SELECT count(*) FROM discussion_events WHERE accepted=0",
                [],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if count >= 1000 && exists.is_none() {
            return Err("pending discussion queue is full".into());
        }
        conn.execute("INSERT OR IGNORE INTO discussion_events (id,entity_id,actor_did,signed_json) VALUES (?1,?2,?3,?4)",params![id,p.entity_id,p.actor_did,signed]).map_err(|e|e.to_string())?;
        return Ok(false);
    }
    super::with_transaction(conn, || {
        match &p.action {
            DiscussionAction::Post { content } => {
                conn.execute("INSERT OR IGNORE INTO discussion_items (id,thread_id,parent_id,subject_field_id,author_did,content_json,body,created_at,revision,event_id,credential_proof_ids) VALUES (?1,?1,NULL,?2,?3,?4,?5,?6,0,?7,?8)",params![p.entity_id,p.subject_field_id,p.actor_did,serde_json::to_string(content).map_err(|e|e.to_string())?,content.body,p.created_at,id,serde_json::to_string(&p.credential_proof_ids).map_err(|e|e.to_string())?]).map_err(|e|e.to_string())?;
            }
            DiscussionAction::Comment { body } => {
                conn.execute("INSERT OR IGNORE INTO discussion_items (id,thread_id,parent_id,subject_field_id,author_did,body,created_at,revision,event_id,credential_proof_ids) VALUES (?1,?2,?3,?4,?5,?6,?7,0,?8,?9)",params![p.entity_id,p.thread_id,p.parent_id,p.subject_field_id,p.actor_did,body,p.created_at,id,serde_json::to_string(&p.credential_proof_ids).map_err(|e|e.to_string())?]).map_err(|e|e.to_string())?;
            }
            DiscussionAction::EditPost { content } => {
                conn.execute("UPDATE discussion_items SET content_json=?2,body=?3,revision=?4,event_id=?5 WHERE id=?1 AND deleted=0 AND (revision<?4 OR (revision=?4 AND event_id<?5))",params![p.entity_id,serde_json::to_string(content).map_err(|e|e.to_string())?,content.body,p.revision,id]).map_err(|e|e.to_string())?;
            }
            DiscussionAction::EditComment { body } => {
                conn.execute("UPDATE discussion_items SET body=?2,revision=?3,event_id=?4 WHERE id=?1 AND deleted=0 AND (revision<?3 OR (revision=?3 AND event_id<?4))",params![p.entity_id,body,p.revision,id]).map_err(|e|e.to_string())?;
            }
            DiscussionAction::Delete => {
                conn.execute(
                    "UPDATE discussion_items SET deleted=1,content_json=NULL,body='' WHERE id=?1",
                    [&p.entity_id],
                )
                .map_err(|e| e.to_string())?;
            }
            DiscussionAction::Vote { value } => {
                conn.execute("INSERT INTO discussion_votes (item_id,actor_did,value,revision,event_id) VALUES (?1,?2,?3,?4,?5) ON CONFLICT(item_id,actor_did) DO UPDATE SET value=excluded.value,revision=excluded.revision,event_id=excluded.event_id WHERE revision<excluded.revision OR (revision=excluded.revision AND event_id<excluded.event_id)",params![p.entity_id,p.actor_did,value,p.revision,id]).map_err(|e|e.to_string())?;
            }
            DiscussionAction::Report { reason } => {
                conn.execute("INSERT OR IGNORE INTO discussion_reports(item_id,actor_did,reason) VALUES (?1,?2,?3)",params![p.entity_id,p.actor_did,reason]).map_err(|e|e.to_string())?;
            }
        }
        conn.execute("INSERT INTO discussion_events (id,entity_id,actor_did,signed_json,accepted) VALUES (?1,?2,?3,?4,1) ON CONFLICT(id) DO UPDATE SET accepted=1",params![id,p.entity_id,p.actor_did,signed]).map_err(|e|e.to_string())?;
        Ok(true)
    })
}

const ITEM_SELECT:&str="SELECT i.id,i.thread_id,i.parent_id,i.subject_field_id,i.author_did,i.content_json,i.body,i.created_at,i.revision,i.deleted, COALESCE((SELECT sum(value) FROM discussion_votes WHERE item_id=i.id),0), COALESCE((SELECT value FROM discussion_votes WHERE item_id=i.id AND actor_did=?1),0), (SELECT count(*) FROM discussion_items c WHERE c.thread_id=i.id AND c.parent_id IS NOT NULL AND c.deleted=0), EXISTS(SELECT 1 FROM discussion_reports WHERE item_id=i.id AND actor_did=?1),i.credential_proof_ids FROM discussion_items i";
fn item_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<DiscussionItem> {
    let content: Option<String> = r.get(5)?;
    let proofs: String = r.get(14)?;
    Ok(DiscussionItem {
        id: r.get(0)?,
        thread_id: r.get(1)?,
        parent_id: r.get(2)?,
        subject_field_id: r.get(3)?,
        author_did: r.get(4)?,
        content: content.and_then(|s| serde_json::from_str(&s).ok()),
        body: r.get(6)?,
        created_at: r.get(7)?,
        edited: r.get::<_, i64>(8)? > 0,
        deleted: r.get::<_, i64>(9)? != 0,
        score: r.get(10)?,
        my_vote: r.get(11)?,
        comment_count: r.get(12)?,
        reported: r.get::<_, i64>(13)? != 0,
        credential_proof_ids: serde_json::from_str(&proofs).unwrap_or_default(),
    })
}
pub fn load_item(
    conn: &Connection,
    id: &str,
    actor: &str,
) -> Result<Option<DiscussionItem>, String> {
    conn.query_row(
        &format!("{ITEM_SELECT} WHERE i.id=?2"),
        params![actor, id],
        item_row,
    )
    .optional()
    .map_err(|e| e.to_string())
}
pub fn list(
    conn: &Connection,
    actor: &str,
    thread: Option<&str>,
    field: Option<&str>,
    sort: &str,
    offset: usize,
) -> Result<Vec<DiscussionItem>, String> {
    let order = match sort {
        "top" => "11 DESC,i.created_at DESC,i.id",
        "discussed" => "13 DESC,i.created_at DESC,i.id",
        _ => "i.created_at DESC,i.id",
    };
    let condition = if thread.is_some() {
        "i.thread_id=?2"
    } else {
        "i.parent_id IS NULL AND i.deleted=0"
    };
    let sql=format!("{ITEM_SELECT} WHERE {condition} AND (?3 IS NULL OR i.subject_field_id=?3) ORDER BY {order} LIMIT 200 OFFSET ?4");
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![actor, thread, field, offset as i64], item_row)
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(rows)
}

pub fn promote(
    conn: &Connection,
    policies: &QualificationPolicySet,
    network: &str,
    now: &str,
) -> Result<(), String> {
    // Multiple passes handle children that arrived before their parents.
    for _ in 0..9 {
        let mut stmt=conn.prepare("SELECT id,signed_json FROM discussion_events WHERE accepted=0 ORDER BY received_at,id LIMIT 1000").map_err(|e|e.to_string())?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        let mut progressed = false;
        for (id, json) in rows {
            let event: DiscussionEvent = serde_json::from_str(&json).map_err(|e| e.to_string())?;
            match ingest(conn, &event, policies, network, now) {
                Ok(true) => progressed = true,
                Err(_) => {
                    conn.execute(
                        "DELETE FROM discussion_events WHERE id=?1 AND accepted=0",
                        [id],
                    )
                    .map_err(|e| e.to_string())?;
                }
                _ => {}
            }
        }
        if !progressed {
            break;
        }
    }
    conn.execute(
        "DELETE FROM discussion_events WHERE accepted=0 AND received_at<unixepoch()-604800",
        [],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn creation_id(p: &DiscussionPayload) -> Result<String, String> {
    let immutable = serde_json_canonicalizer::to_string(&(
        &p.network_id,
        &p.actor_did,
        &p.nonce,
        &p.subject_field_id,
        &p.parent_id,
        p.created_at,
        &p.action,
    ))
    .map_err(|e| e.to_string())?;
    Ok(entity_id(&["discussion-v1", &immutable]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{opinion_eligibility::test_support, Database};
    fn db() -> Database {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        db.conn().execute_batch("INSERT INTO subject_fields(id,name) VALUES ('field','Field'),('other','Other'); INSERT INTO subjects(id,name,subject_field_id) VALUES('subject','Subject','field'); INSERT INTO skills(id,name,subject_id,bloom_level) VALUES('skill','Skill','subject','apply');").unwrap();
        db
    }
    fn post(db: &Database, author: &SigningKey, issuer: &SigningKey) -> DiscussionEvent {
        let did = did_from_verifying_key(&author.verifying_key());
        let credential_id = format!("urn:credential:{}", did.as_str());
        test_support::store_skill_credential(db, &credential_id, issuer, &did, "skill", 2);
        let json: String = db
            .conn()
            .query_row(
                "SELECT signed_vc_json FROM credentials WHERE id=?1",
                [&credential_id],
                |r| r.get(0),
            )
            .unwrap();
        let mut p = DiscussionPayload {
            version: 1,
            network_id: "preprod".into(),
            actor_did: did.as_str().into(),
            entity_id: String::new(),
            thread_id: String::new(),
            parent_id: None,
            subject_field_id: "field".into(),
            nonce: uuid::Uuid::new_v4().to_string(),
            revision: 0,
            created_at: 1_789_430_400,
            credential_proof_ids: vec![credential_id],
            qualification_proofs: vec![DiscussionProof {
                credential: serde_json::from_str(&json).unwrap(),
                endorsement: None,
            }],
            action: DiscussionAction::Post {
                content: PostContent {
                    title: "Should we teach arrays first?".into(),
                    body: "Explain the tradeoffs.".into(),
                    post_kind: "text".into(),
                    url: None,
                    video_cid: None,
                    thumbnail_cid: None,
                },
            },
        };
        p.entity_id = creation_id(&p).unwrap();
        p.thread_id = p.entity_id.clone();
        sign(p, author).unwrap()
    }
    fn change(
        original: &DiscussionEvent,
        key: &SigningKey,
        action: DiscussionAction,
        revision: i64,
    ) -> DiscussionEvent {
        let mut p = original.payload.clone();
        p.actor_did = did_from_verifying_key(&key.verifying_key()).as_str().into();
        p.action = action;
        p.revision = revision;
        sign(p, key).unwrap()
    }
    fn receive(
        db: &Database,
        e: &DiscussionEvent,
        policy: &QualificationPolicySet,
    ) -> Result<bool, String> {
        // Serialize and deserialize the actual wire event: the receiver has no
        // copy of the author's credential database or vault.
        let e: DiscussionEvent = serde_json::from_slice(&serde_json::to_vec(e).unwrap()).unwrap();
        ingest(db.conn(), &e, policy, "preprod", test_support::NOW)
    }
    #[test]
    fn independent_peers_verify_portable_proofs_and_converge_after_reordered_replay() {
        let a = db();
        let b = db();
        let issuer = SigningKey::from_bytes(&[10; 32]);
        let author = SigningKey::from_bytes(&[11; 32]);
        let reader = SigningKey::from_bytes(&[12; 32]);
        let policy = test_support::policy_set(&[&issuer], &["field"]);
        let root = post(&a, &author, &issuer);
        assert!(receive(&a, &root, &policy).unwrap());
        assert!(receive(&b, &root, &policy).unwrap());
        let mut reply = root.payload.clone();
        reply.nonce = "reply".into();
        reply.parent_id = Some(root.payload.entity_id.clone());
        reply.action = DiscussionAction::Comment {
            body: "It depends on the learning goal.".into(),
        };
        reply.entity_id = creation_id(&reply).unwrap();
        let reply = sign(reply, &author).unwrap();
        let up = change(&reply, &reader, DiscussionAction::Vote { value: 1 }, 1);
        let down = change(&reply, &reader, DiscussionAction::Vote { value: -1 }, 2);
        assert!(!receive(&b, &up, &policy).unwrap());
        for peer in [&a, &b] {
            receive(peer, &reply, &policy).unwrap();
            receive(peer, &down, &policy).unwrap();
            receive(peer, &up, &policy).unwrap();
            promote(peer.conn(), &policy, "preprod", test_support::NOW).unwrap();
        }
        let actor = did_from_verifying_key(&reader.verifying_key());
        let first = load_item(a.conn(), &reply.payload.entity_id, actor.as_str())
            .unwrap()
            .unwrap();
        let second = load_item(b.conn(), &reply.payload.entity_id, actor.as_str())
            .unwrap()
            .unwrap();
        assert_eq!(first.score, -1);
        assert_eq!(second.my_vote, -1);
        let delete = change(&reply, &author, DiscussionAction::Delete, 3);
        let edit = change(
            &reply,
            &author,
            DiscussionAction::EditComment {
                body: "An old edit".into(),
            },
            1,
        );
        for peer in [&a, &b] {
            receive(peer, &delete, &policy).unwrap();
            receive(peer, &edit, &policy).unwrap();
            receive(peer, &reply, &policy).unwrap();
            let item = load_item(peer.conn(), &reply.payload.entity_id, actor.as_str())
                .unwrap()
                .unwrap();
            assert!(item.deleted);
            assert!(item.body.is_empty());
        }
        assert_eq!(
            list(a.conn(), actor.as_str(), None, None, "top", 0)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            list(
                b.conn(),
                actor.as_str(),
                Some(&root.payload.entity_id),
                None,
                "new",
                0
            )
            .unwrap()
            .len(),
            2
        );
    }
    #[test]
    fn spoofed_wrong_topic_self_issued_and_unqualified_comments_fail_closed() {
        let a = db();
        let b = db();
        let issuer = SigningKey::from_bytes(&[20; 32]);
        let author = SigningKey::from_bytes(&[21; 32]);
        let stranger = SigningKey::from_bytes(&[22; 32]);
        let policy = test_support::policy_set(&[&issuer], &["field"]);
        let root = post(&a, &author, &issuer);
        receive(&b, &root, &policy).unwrap();
        let mut bad = root.clone();
        bad.payload.actor_did = did_from_verifying_key(&stranger.verifying_key())
            .as_str()
            .into();
        assert!(receive(&b, &bad, &policy).is_err());
        assert!(receive(
            &b,
            &change(&root, &stranger, DiscussionAction::Delete, 1),
            &policy
        )
        .is_err());
        let mut comment = root.payload.clone();
        comment.parent_id = Some(root.payload.entity_id.clone());
        comment.nonce = "no-proof".into();
        comment.action = DiscussionAction::Comment {
            body: "Cannot bypass the gate".into(),
        };
        comment.qualification_proofs.clear();
        comment.credential_proof_ids.clear();
        comment.entity_id = creation_id(&comment).unwrap();
        assert!(receive(&b, &sign(comment, &author).unwrap(), &policy).is_err());
        let own = post(&a, &stranger, &stranger);
        assert!(receive(&b, &own, &policy).is_err());
        let mut wrong = root.payload.clone();
        wrong.subject_field_id = "other".into();
        wrong.entity_id = creation_id(&wrong).unwrap();
        wrong.thread_id = wrong.entity_id.clone();
        assert!(receive(&b, &sign(wrong, &author).unwrap(), &policy).is_err());
        let mut tampered = root.clone();
        tampered.payload.qualification_proofs[0].credential.id = Some("forged".into());
        assert!(receive(&b, &tampered, &policy).is_err());
    }
    #[test]
    fn votes_can_be_removed_reports_do_not_delete_and_edits_converge() {
        let a = db();
        let b = db();
        let issuer = SigningKey::from_bytes(&[30; 32]);
        let author = SigningKey::from_bytes(&[31; 32]);
        let reader = SigningKey::from_bytes(&[32; 32]);
        let policy = test_support::policy_set(&[&issuer], &["field"]);
        let root = post(&a, &author, &issuer);
        let mut content = match &root.payload.action {
            DiscussionAction::Post { content } => content.clone(),
            _ => unreachable!(),
        };
        content.title = "Updated question".into();
        let edit = change(&root, &author, DiscussionAction::EditPost { content }, 1);
        let report = change(
            &root,
            &reader,
            DiscussionAction::Report {
                reason: "off_topic".into(),
            },
            1,
        );
        let up = change(&root, &reader, DiscussionAction::Vote { value: 1 }, 2);
        let clear = change(&root, &reader, DiscussionAction::Vote { value: 0 }, 3);
        for event in [&root, &edit, &report, &up, &clear] {
            receive(&a, event, &policy).unwrap();
        }
        for event in [&clear, &up, &report, &edit, &root] {
            receive(&b, event, &policy).unwrap();
        }
        promote(b.conn(), &policy, "preprod", test_support::NOW).unwrap();
        let actor = did_from_verifying_key(&reader.verifying_key());
        for peer in [&a, &b] {
            let item = load_item(peer.conn(), &root.payload.entity_id, actor.as_str())
                .unwrap()
                .unwrap();
            assert!(!item.deleted);
            assert!(item.reported);
            assert_eq!(item.score, 0);
            assert_eq!(item.content.unwrap().title, "Updated question");
        }
    }
}
