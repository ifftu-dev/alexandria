use rusqlite::params;

use crate::crypto::{did::did_from_verifying_key, hash::entity_id, wallet};
use crate::db::{discussions, executor::DatabaseWorkload, opinion_eligibility};
use crate::domain::discussions::*;
use crate::network_profile::{embedded_preprod, embedded_qualification_policies};
use crate::profile::scope::ProfileState as State;
use crate::AppState;

async fn identity(state: &AppState) -> Result<wallet::Wallet, String> {
    let guard = state.keystore.lock().await;
    let mnemonic = guard
        .as_ref()
        .ok_or("profile is locked")?
        .retrieve_mnemonic()
        .map_err(|e| e.to_string())?;
    wallet::wallet_from_mnemonic(&mnemonic).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn discussion_access(state: State<'_, AppState>) -> Result<DiscussionAccess, String> {
    let w = identity(&state).await?;
    let did = did_from_verifying_key(&w.signing_key.verifying_key());
    let policies = embedded_qualification_policies().map_err(|e| e.to_string())?;
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "discussion.access",
            move |db| {
                Ok(DiscussionAccess {
                    actor_did: did.as_str().to_owned(),
                    eligible_fields: opinion_eligibility::eligible_opinion_subject_fields(
                        db.conn(),
                        policies,
                        &did,
                        &opinion_eligibility::verification_time_now(),
                    )?,
                    governed_fields: policies
                        .policies()
                        .iter()
                        .flat_map(|p| p.policy().subject_field_ids.clone())
                        .collect(),
                })
            },
        )
        .await
}

#[tauri::command]
pub async fn list_discussions(
    state: State<'_, AppState>,
    thread_id: Option<String>,
    subject_field_id: Option<String>,
    sort: Option<String>,
    offset: Option<usize>,
) -> Result<Vec<DiscussionItem>, String> {
    let w = identity(&state).await?;
    let actor = did_from_verifying_key(&w.signing_key.verifying_key())
        .as_str()
        .to_owned();
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "discussion.list",
            move |db| {
                discussions::list(
                    db.conn(),
                    &actor,
                    thread_id.as_deref(),
                    subject_field_id.as_deref(),
                    sort.as_deref().unwrap_or("new"),
                    offset.unwrap_or(0).min(1_000_000),
                )
            },
        )
        .await
}

#[tauri::command]
pub async fn act_on_discussion(
    state: State<'_, AppState>,
    req: DiscussionRequest,
) -> Result<String, String> {
    let w = identity(&state).await?;
    let address = w.stake_address.clone();
    let key = w.signing_key.clone();
    let actor = did_from_verifying_key(&key.verifying_key());
    let policies = embedded_qualification_policies().map_err(|e| e.to_string())?;
    let network = embedded_preprod()
        .map_err(|e| e.to_string())?
        .network_id
        .clone();
    let (id,event)=state.db_executor.execute(DatabaseWorkload::Instructor,state.profile_lease(),"discussion.act",move|db| {
        let conn=db.conn();
        let nonce=uuid::Uuid::new_v4().to_string();
        let creating=matches!(req.action,DiscussionAction::Post{..}|DiscussionAction::Comment{..});
        let id=if creating {entity_id(&["discussion-v1",actor.as_str(),&nonce])}else{req.entity_id.clone().ok_or("missing target")?};
        let thread_id=if matches!(req.action,DiscussionAction::Post{..}) {id.clone()}else{req.thread_id.clone().ok_or("missing thread")?};
        if let Some(root)=discussions::load_item(conn,&thread_id,actor.as_str())? {
            if root.deleted && !matches!(req.action,DiscussionAction::Delete|DiscussionAction::Report{..}) {return Err("this thread has been deleted".into());}
        }
        let now=opinion_eligibility::verification_time_now();
        let mut proofs=Vec::new();
        let mut qualification_proofs=Vec::new();
        if creating || matches!(req.action,DiscussionAction::EditPost{..}|DiscussionAction::EditComment{..}) {
            let mut stmt=conn.prepare("SELECT id FROM credentials WHERE subject_did=?1 AND claim_kind='skill' AND revoked=0 ORDER BY id").map_err(|e|e.to_string())?;
            let candidates=stmt.query_map([actor.as_str()],|r|r.get::<_,String>(0)).map_err(|e|e.to_string())?.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?;
            for proof in candidates {
                if matches!(opinion_eligibility::check_opinion_credential(conn,policies,&proof,&actor,&req.subject_field_id,&now)?,opinion_eligibility::OpinionCredentialEligibility::Qualified(_)) {
                    let json:String=conn.query_row("SELECT signed_vc_json FROM credentials WHERE id=?1",[&proof],|r|r.get(0)).map_err(|e|e.to_string())?;
                    let stored=crate::commands::attestation::stored_completion_evidence(conn,&proof)?;
                    let endorsement=stored.as_ref().map(|s|{let e=s.as_evidence();DiscussionEndorsement{policy:e.policy.clone(),binding:e.binding.clone(),endorsements:e.endorsements.to_vec()}});
                    qualification_proofs.push(DiscussionProof{credential:serde_json::from_str(&json).map_err(|e|e.to_string())?,endorsement});
                    proofs.push(proof);break;
                }
            }
        }
        let revision=if creating {0}else{
            // Lamport revision per actor/target, persisted across restarts.
            conn.query_row("SELECT COALESCE(MAX(json_extract(signed_json,'$.payload.revision')),0)+1 FROM discussion_events WHERE entity_id=?1 AND actor_did=?2 AND accepted=1",params![id,actor.as_str()],|r|r.get::<_,i64>(0)).map_err(|e|e.to_string())?
        };
        let mut payload=DiscussionPayload{version:1,network_id:network.clone(),actor_did:actor.as_str().to_owned(),entity_id:id.clone(),thread_id,parent_id:req.parent_id,subject_field_id:req.subject_field_id,nonce,revision,created_at:chrono::Utc::now().timestamp(),credential_proof_ids:proofs,qualification_proofs,action:req.action};
        let id = if creating {discussions::creation_id(&payload)?} else {id};
        payload.entity_id=id.clone();
        if matches!(payload.action, DiscussionAction::Post{..}) {payload.thread_id=id.clone();}
        let event=discussions::sign(payload,&key)?;
        let wire = crate::p2p::signing::sign_gossip_message(crate::p2p::types::TOPIC_OPINIONS,serde_json::to_vec(&event).map_err(|e|e.to_string())?,&key,&address);
        if serde_json::to_vec(&wire).map_err(|e|e.to_string())?.len()>crate::p2p::types::MAX_GOSSIP_MESSAGE_BYTES {return Err("Post and qualification proof exceed the network message limit; shorten the text.".into());}
        // Local writes must be immediately valid; never report success for a pending action.
        crate::db::with_transaction(conn,|| {
            if !discussions::ingest(conn,&event,policies,&network,&now)? {return Err("thread dependencies have not arrived yet".into());}
            Ok(())
        })?;
        Ok((id,event))
    }).await?;
    if let Some(node) = state.p2p_node.lock().await.as_ref() {
        if let Err(e) = node
            .publish_opinion(
                serde_json::to_vec(&event).map_err(|e| e.to_string())?,
                &w.signing_key,
                &w.stake_address,
            )
            .await
        {
            log::debug!("discussion queued for retry: {e}");
        }
    }
    Ok(id)
}

pub(crate) async fn start_relay(state: &AppState) -> Result<(), String> {
    let w = identity(state).await?;
    let db = crate::p2p::inbound::InboundDatabase::pin_on_first_use(
        state.db_executor.clone(),
        state.profile_operations.clone(),
    );
    let node = state.p2p_node.clone();
    state.profile_operations.spawn_job(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            let batch=db.run("discussion.retry",|db| {
                let policies=embedded_qualification_policies().map_err(|e|e.to_string())?;
                let network=&embedded_preprod().map_err(|e|e.to_string())?.network_id;
                discussions::promote(db.conn(),policies,network,&opinion_eligibility::verification_time_now())?;
                let mut stmt=db.conn().prepare("SELECT id,signed_json FROM discussion_events WHERE accepted=1 ORDER BY last_shared,id LIMIT 16").map_err(|e|e.to_string())?;
                let rows=stmt.query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?))).map_err(|e|e.to_string())?.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?;
                Ok(rows)
            }).await;
            let Ok(batch)=batch else {continue};
            for (id,json) in batch {
                let guard=node.lock().await;
                let Some(node)=guard.as_ref() else {break};
                if node.publish_opinion(json.into_bytes(),&w.signing_key,&w.stake_address).await.is_err() {break;}
                drop(guard);
                let _=db.run("discussion.shared",move|db| db.conn().execute("UPDATE discussion_events SET last_shared=unixepoch() WHERE id=?1",[id]).map(drop).map_err(|e|e.to_string())).await;
            }
        }
    }).await;
    Ok(())
}

#[tauri::command]
pub async fn discussion_add_media(
    state: State<'_, AppState>,
    data: Vec<u8>,
    media_kind: String,
) -> Result<crate::content_store::content::AddResult, String> {
    use crate::content_store::{content, storage};
    let image = data.starts_with(b"\x89PNG\r\n\x1a\n")
        || data.starts_with(b"\xff\xd8\xff")
        || (data.starts_with(b"RIFF") && data.get(8..12) == Some(b"WEBP"));
    let video = data.get(4..8) == Some(b"ftyp") || data.starts_with(b"\x1a\x45\xdf\xa3");
    if data.is_empty()
        || data.len() > 25 * 1024 * 1024
        || !matches!(
            (media_kind.as_str(), image, video),
            ("image", true, _) | ("video", _, true)
        )
    {
        return Err("choose a PNG/JPEG/WebP image or MP4/WebM video, up to 25 MB".into());
    }
    let result = content::add_bytes_unencrypted(&state.content_node, &data)
        .await
        .map_err(|e| e.to_string())?;
    let hash = result.hash.clone();
    let size = result.size;
    state
        .db_executor
        .execute(
            DatabaseWorkload::Instructor,
            state.profile_lease(),
            "discussion.media",
            move |db| storage::upsert_pin(db.conn(), &hash, "opinion", size, false),
        )
        .await?;
    if let (Ok(hash), Some(endpoint)) = (
        content::parse_hash(&result.hash),
        state.content_node.endpoint().await,
    ) {
        state
            .discovery
            .announce_have(hash, &endpoint)
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(result)
}
