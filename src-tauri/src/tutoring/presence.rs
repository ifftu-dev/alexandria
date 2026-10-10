//! Room-scoped signed attendance leases. Observers never count as attendees.
use std::collections::HashMap;
use std::time::Duration;

use bytes::Bytes;
use ed25519_dalek::{Signature, VerifyingKey};
use futures::StreamExt;
use iroh_gossip::api::{Event, GossipSender};
use live::rooms::RoomTicket;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio::task::JoinSet;

use crate::db::executor::DatabaseWorkload;
use crate::profile::scope::ProfileState;
use crate::AppState;

pub const EMPTY_SECONDS: i64 = 300;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Presence {
    room: String,
    node: String,
    present: bool,
    at: i64,
    signature: String,
}
fn room_id(ticket: &RoomTicket) -> String {
    hex::encode(ticket.topic_id.as_bytes())
}
fn topic(room: &str) -> iroh_gossip::proto::TopicId {
    iroh_gossip::proto::TopicId::from_bytes(
        *blake3::hash(format!("alexandria-tutoring-presence-v1:{room}").as_bytes()).as_bytes(),
    )
}
fn bytes(p: &Presence) -> Vec<u8> {
    format!(
        "alexandria-tutoring-presence-v1\n{}\n{}\n{}\n{}",
        p.room, p.node, p.present, p.at
    )
    .into_bytes()
}
fn validate(p: &Presence, room: &str, now: i64) -> Result<(), String> {
    if p.room != room || p.at > now + 10 || p.at < now - 45 {
        return Err("stale room presence".into());
    }
    let key: [u8; 32] = hex::decode(&p.node)
        .map_err(|e| e.to_string())?
        .try_into()
        .map_err(|_| "invalid room node")?;
    let key = VerifyingKey::from_bytes(&key).map_err(|e| e.to_string())?;
    let sig = Signature::from_slice(&hex::decode(&p.signature).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    key.verify_strict(&bytes(p), &sig)
        .map_err(|e| e.to_string())
}
fn apply(conn: &Connection, p: &Presence) -> Result<(), String> {
    conn.execute("INSERT INTO tutoring_presence(room_id,node_id,present,seen_at) VALUES(?1,?2,?3,?4) ON CONFLICT(room_id,node_id) DO UPDATE SET present=excluded.present,seen_at=excluded.seen_at WHERE seen_at<excluded.seen_at",params![p.room,p.node,p.present,p.at]).map_err(|e|e.to_string())?;
    // Departure starts the grace period too; another attendee's next heartbeat
    // keeps the room alive. The observer itself never renews this timestamp.
    conn.execute("UPDATE tutoring_sessions SET last_occupied_at=MAX(COALESCE(last_occupied_at,0),?2) WHERE room_id=?1 AND status='active'",params![p.room,p.at]).map_err(|e|e.to_string())?;
    Ok(())
}
pub(crate) fn expire(conn: &Connection, now: i64) -> Result<(), String> {
    conn.execute("UPDATE tutoring_sessions SET status='ended',ended_at=datetime(?1,'unixepoch') WHERE status='active' AND COALESCE(last_occupied_at,unixepoch(created_at),0)<=?1-?2",params![now,EMPTY_SECONDS]).map_err(|e|e.to_string())?;
    Ok(())
}

pub(crate) async fn check_join(
    state: &ProfileState<'_, AppState>,
    ticket: &str,
) -> Result<(), String> {
    let parsed: RoomTicket = ticket
        .parse()
        .map_err(|e| format!("invalid room ticket: {e}"))?;
    let room = room_id(&parsed);
    let lookup = room.clone();
    let known=state.db_executor.execute(DatabaseWorkload::Learner,state.profile_lease(),"tutoring.check-join",move|db| {
        db.conn().query_row("SELECT status,COALESCE(last_occupied_at,unixepoch(created_at),0) FROM tutoring_sessions WHERE room_id=?1 ORDER BY created_at DESC LIMIT 1",[lookup],|r|Ok((r.get::<_,String>(0)?,r.get::<_,i64>(1)?))).optional().map_err(|e|e.to_string())
    }).await?;
    if let Some((status, last)) = &known {
        if status != "active" {
            return Err(
                "SESSION_ENDED: This tutoring session has ended. Ask for a new session invite."
                    .into(),
            );
        }
        if *last > chrono::Utc::now().timestamp() - EMPTY_SECONDS {
            return Ok(());
        }
    }
    // An unfamiliar ticket is not permission to create a new empty room.
    // Wait for a signed attendance heartbeat before opening camera or mic.
    let gossip = state
        .content_node
        .gossip()
        .await
        .ok_or("gossip not available")?;
    let subscription = gossip
        .subscribe(topic(&room), parsed.bootstrap)
        .await
        .map_err(|e| e.to_string())?;
    let (_sender, mut receiver) = subscription.split();
    let found = tokio::time::timeout(Duration::from_secs(22), async {
        while let Some(Ok(event)) = receiver.next().await {
            if let Event::Received(message) = event {
                if message.content.len() > 2048 {
                    continue;
                }
                if let Ok(p) = serde_json::from_slice::<Presence>(&message.content) {
                    if p.present && validate(&p, &room, chrono::Utc::now().timestamp()).is_ok() {
                        return true;
                    }
                }
            }
        }
        false
    })
    .await
    .unwrap_or(false);
    if found {
        Ok(())
    } else {
        Err("SESSION_UNAVAILABLE: No participant is reachable. This session may have ended; ask the host for a current invite.".into())
    }
}

pub(crate) fn register(conn: &Connection, id: &str, ticket: &str) -> Result<(), String> {
    let parsed: RoomTicket = ticket
        .parse()
        .map_err(|e| format!("invalid room ticket: {e}"))?;
    conn.execute(
        "UPDATE tutoring_sessions SET room_id=?2,last_occupied_at=unixepoch() WHERE id=?1",
        params![id, room_id(&parsed)],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub(crate) async fn start(state: &AppState) -> Result<(), String> {
    let endpoint = state
        .content_node
        .endpoint()
        .await
        .ok_or("iroh endpoint unavailable")?;
    let gossip = state
        .content_node
        .gossip()
        .await
        .ok_or("iroh gossip unavailable")?;
    let db = crate::p2p::inbound::InboundDatabase::pin_on_first_use(
        state.db_executor.clone(),
        state.profile_operations.clone(),
    );
    let manager = state.tutoring.clone();
    state.profile_operations.spawn_job(async move {
        let mut subscriptions:HashMap<String,(GossipSender,bool,tokio::task::AbortHandle)>=HashMap::new();
        let mut listeners=JoinSet::new();
        let (tx,mut rx)=mpsc::channel::<Presence>(128);
        let started=std::time::Instant::now();
        let mut tick=tokio::time::interval(Duration::from_secs(10));
        loop {
            tokio::select! {
                Some(p)=rx.recv()=> {
                    let _=db.run("tutoring.presence",move|db|apply(db.conn(),&p)).await;
                },
                _=tick.tick()=> {
                    let active=manager.status().await;
                    let active_room=active.as_ref().and_then(|s|s.ticket.parse::<RoomTicket>().ok()).map(|t|room_id(&t));
                    let active_id=active.as_ref().map(|s|s.session_id.clone());
                    let reconcile_expiry=started.elapsed()>=Duration::from_secs(30);
                    let rooms=db.run("tutoring.presence-rooms",move|db| {
                        let conn=db.conn();
                        if let Some(id)=active_id {conn.execute("UPDATE tutoring_sessions SET last_occupied_at=unixepoch() WHERE id=?1 AND status='active'",[id]).map_err(|e|e.to_string())?;}
                        let mut stmt=conn.prepare("SELECT id,ticket FROM tutoring_sessions WHERE status='active' AND ticket IS NOT NULL ORDER BY created_at DESC").map_err(|e|e.to_string())?;
                        let rows=stmt.query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?))).map_err(|e|e.to_string())?.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?;
                        for (id,ticket) in &rows {
                            if let Ok(parsed)=ticket.parse::<RoomTicket>() {conn.execute("UPDATE tutoring_sessions SET room_id=?2,last_occupied_at=COALESCE(last_occupied_at,unixepoch(created_at)) WHERE id=?1",params![id,room_id(&parsed)]).map_err(|e|e.to_string())?;}
                        }
                        if reconcile_expiry {expire(conn,chrono::Utc::now().timestamp())?;}
                        let rows=rows.into_iter().filter(|(id,_)|conn.query_row("SELECT status='active' FROM tutoring_sessions WHERE id=?1",[id],|r|r.get::<_,bool>(0)).unwrap_or(false)).collect::<Vec<_>>();
                        Ok(rows)
                    }).await;
                    let Ok(rooms)=rooms else{continue};
                    let wanted:std::collections::HashSet<String>=rooms.iter().filter_map(|(_,ticket)|ticket.parse::<RoomTicket>().ok()).map(|t|room_id(&t)).collect();
                    subscriptions.retain(|room,(_,_,task)| {if wanted.contains(room) {true} else {task.abort();false}});
                    for (_,ticket) in rooms {
                        let Ok(parsed)=ticket.parse::<RoomTicket>() else{continue};
                        let room=room_id(&parsed);
                        if subscriptions.contains_key(&room) {continue;}
                        let Ok(subscription)=gossip.subscribe(topic(&room),parsed.bootstrap).await else{continue};
                        let (sender,mut receiver)=subscription.split();let tx=tx.clone();let observed=room.clone();
                        let task=listeners.spawn(async move {
                            while let Some(Ok(event))=receiver.next().await {
                                if let Event::Received(message)=event {
                                    if message.content.len()>2048 {continue;}
                                    if let Ok(p)=serde_json::from_slice::<Presence>(&message.content) {
                                        if validate(&p,&observed,chrono::Utc::now().timestamp()).is_ok() {let _=tx.try_send(p);}
                                    }
                                }
                            }
                        });
                        subscriptions.insert(room,(sender,false,task));
                    }
                    for (room,(sender,was_present,_)) in &mut subscriptions {
                        let here=active_room.as_ref()==Some(room);
                        if here || *was_present {
                            if here {
                                if let Some(active)=&active {
                                    let peers=active.peers.iter().filter_map(|p|p.node_id.parse().ok()).collect();
                                    let _=sender.join_peers(peers).await;
                                }
                            }
                            let mut p=Presence{room:room.clone(),node:endpoint.id().to_string(),present:here,at:chrono::Utc::now().timestamp(),signature:String::new()};
                            p.signature=hex::encode(endpoint.secret_key().sign(&bytes(&p)).to_bytes());
                            if let Ok(json)=serde_json::to_vec(&p) {let _=sender.broadcast(Bytes::from(json)).await;}
                            let _=db.run("tutoring.own-presence",move|db|apply(db.conn(),&p)).await;
                        }
                        *was_present=here;
                    }
                    while listeners.try_join_next().is_some() {}
                }
            }
        }
    }).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn empty_rooms_expire_at_five_minutes_and_presence_keeps_occupied_rooms_alive() {
        let db = crate::db::Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        db.conn().execute_batch("INSERT INTO tutoring_sessions(id,title,status,room_id,last_occupied_at) VALUES('empty','Empty','active','room',100),('occupied','Occupied','active','other',100);").unwrap();
        expire(db.conn(), 399).unwrap();
        let status = |id: &str| {
            db.conn()
                .query_row(
                    "SELECT status FROM tutoring_sessions WHERE id=?1",
                    [id],
                    |r| r.get::<_, String>(0),
                )
                .unwrap()
        };
        assert_eq!(status("empty"), "active");
        apply(
            db.conn(),
            &Presence {
                room: "other".into(),
                node: "peer".into(),
                present: true,
                at: 399,
                signature: String::new(),
            },
        )
        .unwrap();
        expire(db.conn(), 400).unwrap();
        assert_eq!(status("empty"), "ended");
        assert_eq!(status("occupied"), "active");
        expire(db.conn(), 699).unwrap();
        assert_eq!(status("occupied"), "ended");
    }
    #[test]
    fn presence_is_room_bound_signed_and_short_lived() {
        let key = iroh::SecretKey::generate();
        let mut p = Presence {
            room: "room".into(),
            node: key.public().to_string(),
            present: true,
            at: 100,
            signature: String::new(),
        };
        p.signature = hex::encode(key.sign(&bytes(&p)).to_bytes());
        assert!(validate(&p, "room", 110).is_ok());
        assert!(validate(&p, "other", 110).is_err());
        assert!(validate(&p, "room", 146).is_err());
        p.present = false;
        assert!(validate(&p, "room", 110).is_err());
    }
}
