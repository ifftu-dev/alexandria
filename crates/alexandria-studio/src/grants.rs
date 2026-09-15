use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StudioGrant {
    pub id: String,
    pub client_name: String,
    pub scopes: Vec<String>,
    pub expires_at: i64,
}

#[derive(Default)]
pub struct Grants {
    entries: HashMap<String, (StudioGrant, String, u64)>,
}
impl Grants {
    pub fn issue(
        &mut self,
        name: String,
        scopes: Vec<String>,
        epoch: u64,
        now: i64,
    ) -> Result<(StudioGrant, String)> {
        if name.trim().is_empty()
            || name.len() > 100
            || scopes.is_empty()
            || scopes.len() > 3
            || scopes
                .iter()
                .any(|s| !["learning:read", "drafts:read", "drafts:propose"].contains(&s.as_str()))
        {
            return Err(Error::Invalid(
                "Choose a client name and supported permissions".into(),
            ));
        }
        self.entries
            .retain(|_, (grant, _, _)| grant.expires_at > now);
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
            (
                grant.clone(),
                blake3::hash(token.as_bytes()).to_hex().to_string(),
                epoch,
            ),
        );
        Ok((grant, token))
    }
    pub fn authorize(&self, token: &str, scope: &str, epoch: u64, now: i64) -> Result<StudioGrant> {
        let hash = blake3::hash(token.as_bytes()).to_hex().to_string();
        self.entries
            .values()
            .find(|(g, h, e)| {
                h == &hash
                    && *e == epoch
                    && g.expires_at > now
                    && g.scopes.iter().any(|s| s == scope)
            })
            .map(|(g, _, _)| g.clone())
            .ok_or(Error::Permission)
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
            .filter(|(g, _, _)| g.expires_at > now)
            .map(|(g, _, _)| g.clone())
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
}
