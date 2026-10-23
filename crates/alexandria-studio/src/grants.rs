//! Grants for external assistants, held in memory for the unlocked profile.
//!
//! A grant names the scopes an assistant may use, expires after an hour, and
//! is bound to the profile epoch, so a lock or a profile switch ends it. Each
//! grant keeps a short log of what the assistant read and a budget for
//! fetching lessons from peers; both vanish with the grant.

use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};

/// Every scope a grant may carry.
pub const SCOPES: [&str; 6] = [
    "learning:read",
    // Include the learner's private skills in the skill graph. Only with
    // learning:read; off unless chosen.
    "learning:private_skills",
    "credentials:read",
    // Verify a presentation. Separate from credentials:read because it
    // records the presentation's nonce on this device.
    "credentials:verify",
    "drafts:read",
    "drafts:propose",
];

/// Lessons a grant may fetch from peers per hour.
pub const FETCHES_PER_HOUR: usize = 20;
/// Entries kept in each grant's activity log.
pub const ACTIVITY_ENTRIES: usize = 50;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StudioGrant {
    pub id: String,
    pub client_name: String,
    pub scopes: Vec<String>,
    pub expires_at: i64,
}

impl StudioGrant {
    pub fn has(&self, scope: &str) -> bool {
        self.scopes.iter().any(|s| s == scope)
    }
}

/// One request an assistant made, as shown to the learner.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Activity {
    pub at: i64,
    pub tool: String,
    /// What it asked about: a course and lesson, a search, or nothing.
    pub detail: String,
    /// `returned`, `withheld`, `unavailable`, or a refusal code.
    pub outcome: String,
}

struct Entry {
    grant: StudioGrant,
    token_hash: String,
    epoch: u64,
    activity: VecDeque<Activity>,
    fetches: VecDeque<i64>,
}

#[derive(Default)]
pub struct Grants {
    entries: HashMap<String, Entry>,
}

impl Grants {
    pub fn issue(
        &mut self,
        name: String,
        scopes: Vec<String>,
        epoch: u64,
        now: i64,
    ) -> Result<(StudioGrant, String)> {
        let invalid = || Error::Invalid("Choose a client name and supported permissions".into());
        if name.trim().is_empty()
            || name.len() > 100
            || scopes.is_empty()
            || scopes.len() > SCOPES.len()
            || scopes.iter().any(|s| !SCOPES.contains(&s.as_str()))
        {
            return Err(invalid());
        }
        if scopes.iter().any(|s| s == "learning:private_skills")
            && !scopes.iter().any(|s| s == "learning:read")
        {
            return Err(invalid());
        }
        self.entries.retain(|_, e| e.grant.expires_at > now);
        if self.entries.len() >= 20 {
            return Err(Error::Invalid(
                "Revoke an existing assistant before adding another".into(),
            ));
        }
        let token = format!("{}{}", uuid::Uuid::new_v4(), uuid::Uuid::new_v4());
        let grant = StudioGrant {
            id: uuid::Uuid::new_v4().to_string(),
            client_name: name,
            scopes,
            expires_at: now + 3600,
        };
        self.entries.insert(
            grant.id.clone(),
            Entry {
                grant: grant.clone(),
                token_hash: blake3::hash(token.as_bytes()).to_hex().to_string(),
                epoch,
                activity: VecDeque::new(),
                fetches: VecDeque::new(),
            },
        );
        Ok((grant, token))
    }

    pub fn authorize(&self, token: &str, scope: &str, epoch: u64, now: i64) -> Result<StudioGrant> {
        let hash = blake3::hash(token.as_bytes()).to_hex().to_string();
        self.entries
            .values()
            .find(|e| {
                e.token_hash == hash
                    && e.epoch == epoch
                    && e.grant.expires_at > now
                    && e.grant.has(scope)
            })
            .map(|e| e.grant.clone())
            .ok_or(Error::Permission)
    }

    /// Records a request in the grant's activity log.
    pub fn record(&mut self, grant_id: &str, activity: Activity) {
        if let Some(entry) = self.entries.get_mut(grant_id) {
            if entry.activity.len() >= ACTIVITY_ENTRIES {
                entry.activity.pop_front();
            }
            entry.activity.push_back(activity);
        }
    }

    /// Spends one peer fetch from the grant's hourly budget. False when the
    /// budget is used up.
    pub fn take_fetch(&mut self, grant_id: &str, now: i64) -> bool {
        let Some(entry) = self.entries.get_mut(grant_id) else {
            return false;
        };
        while entry.fetches.front().is_some_and(|t| *t <= now - 3600) {
            entry.fetches.pop_front();
        }
        if entry.fetches.len() >= FETCHES_PER_HOUR {
            return false;
        }
        entry.fetches.push_back(now);
        true
    }

    /// The grant's log, newest first.
    pub fn activity(&self, grant_id: &str) -> Vec<Activity> {
        self.entries
            .get(grant_id)
            .map(|e| e.activity.iter().rev().cloned().collect())
            .unwrap_or_default()
    }

    pub fn revoke(&mut self, id: &str) {
        self.entries.remove(id);
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn list(&self, now: i64) -> Vec<StudioGrant> {
        self.entries
            .values()
            .filter(|e| e.grant.expires_at > now)
            .map(|e| e.grant.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn grants_are_scoped_expiring_revocable_and_bound_to_profile_epoch() {
        let mut grants = Grants::default();
        let (a, token) = grants
            .issue("Read assistant".into(), vec!["drafts:read".into()], 1, 100)
            .unwrap();
        assert!(grants.authorize(&token, "drafts:read", 1, 200).is_ok());
        for (scope, epoch, time) in [
            ("drafts:propose", 1, 200),
            ("drafts:read", 2, 200),
            ("drafts:read", 1, 3700),
        ] {
            assert!(grants.authorize(&token, scope, epoch, time).is_err());
        }
        assert!(grants.authorize("wrong", "drafts:read", 1, 200).is_err());
        grants.revoke(&a.id);
        assert!(grants.authorize(&token, "drafts:read", 1, 200).is_err());
        let (_, token) = grants
            .issue("Other".into(), vec!["drafts:read".into()], 3, 200)
            .unwrap();
        grants.clear();
        assert!(grants.authorize(&token, "drafts:read", 3, 201).is_err());
    }

    #[test]
    fn private_skills_need_learning_and_verification_is_its_own_scope() {
        let mut grants = Grants::default();
        assert!(grants
            .issue("A".into(), vec!["learning:private_skills".into()], 1, 0)
            .is_err());
        let (g, token) = grants
            .issue(
                "A".into(),
                vec!["learning:read".into(), "learning:private_skills".into()],
                1,
                0,
            )
            .unwrap();
        assert!(g.has("learning:private_skills"));
        let (_, reader) = grants
            .issue("B".into(), vec!["credentials:read".into()], 1, 0)
            .unwrap();
        assert!(grants
            .authorize(&reader, "credentials:verify", 1, 1)
            .is_err());
        assert!(grants.authorize(&token, "learning:read", 1, 1).is_ok());
        assert!(grants
            .issue("C".into(), vec!["admin".into()], 1, 0)
            .is_err());
    }

    #[test]
    fn the_fetch_budget_refills_after_an_hour_and_the_log_is_bounded() {
        let mut grants = Grants::default();
        let (g, _) = grants
            .issue("A".into(), vec!["learning:read".into()], 1, 0)
            .unwrap();
        for i in 0..FETCHES_PER_HOUR {
            assert!(grants.take_fetch(&g.id, i as i64));
        }
        assert!(!grants.take_fetch(&g.id, 100));
        assert!(grants.take_fetch(&g.id, 3601));
        for i in 0..(ACTIVITY_ENTRIES + 5) {
            grants.record(
                &g.id,
                Activity {
                    at: i as i64,
                    tool: "read_lesson".into(),
                    detail: String::new(),
                    outcome: "returned".into(),
                },
            );
        }
        let log = grants.activity(&g.id);
        assert_eq!(log.len(), ACTIVITY_ENTRIES);
        assert_eq!(log[0].at, (ACTIVITY_ENTRIES + 4) as i64, "newest first");
        assert!(grants.activity("missing").is_empty());
    }
}
