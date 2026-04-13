//! IPC commands for Verifiable Credentials. Stubs — PR 5.

use tauri::State;

use crate::crypto::did::Did;
use crate::domain::vc::{VerifiableCredential, VerificationResult};
use crate::AppState;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct IssueCredentialRequest {
    pub credential_type: crate::domain::vc::CredentialType,
    pub subject: Did,
    pub claim: crate::domain::vc::Claim,
    pub evidence_refs: Vec<String>,
    pub expiration_date: Option<String>,
}

#[tauri::command]
pub async fn issue_credential(
    _state: State<'_, AppState>,
    _req: IssueCredentialRequest,
) -> Result<VerifiableCredential, String> {
    Err("PR 5 — issue_credential not yet implemented".into())
}

#[tauri::command]
pub async fn list_credentials(
    _state: State<'_, AppState>,
    _subject: Option<String>,
    _skill_id: Option<String>,
) -> Result<Vec<VerifiableCredential>, String> {
    Err("PR 5 — list_credentials not yet implemented".into())
}

#[tauri::command]
pub async fn get_credential(
    _state: State<'_, AppState>,
    _credential_id: String,
) -> Result<Option<VerifiableCredential>, String> {
    Err("PR 5 — get_credential not yet implemented".into())
}

#[tauri::command]
pub async fn revoke_credential(
    _state: State<'_, AppState>,
    _credential_id: String,
    _reason: String,
) -> Result<(), String> {
    Err("PR 5 — revoke_credential not yet implemented".into())
}

#[tauri::command]
pub async fn verify_credential_cmd(
    _state: State<'_, AppState>,
    _credential: VerifiableCredential,
) -> Result<VerificationResult, String> {
    Err("PR 4 — verify_credential_cmd not yet implemented".into())
}

// ---------------------------------------------------------------------------
// Unit tests. Command handlers themselves are thin wrappers around domain
// functions and are exercised via integration tests in their implementation
// PRs (constructing a `State<AppState>` in isolation isn't ergonomic in
// unit tests). What *does* belong here is the serde shape of IPC request
// structs — the frontend deserializes JSON into these and a silent rename
// would fail on the UI boundary.
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::vc::{Claim, CredentialType, SkillClaim};

    #[test]
    fn issue_credential_request_round_trips_through_serde() {
        let req = IssueCredentialRequest {
            credential_type: CredentialType::FormalCredential,
            subject: Did("did:key:zSubject".into()),
            claim: Claim::Skill(SkillClaim {
                skill_id: "skill_x".into(),
                level: 4,
                score: 0.8,
                evidence_refs: vec!["urn:uuid:e1".into()],
                rubric_version: Some("v1".into()),
                assessment_method: Some("exam".into()),
            }),
            evidence_refs: vec!["urn:uuid:e1".into()],
            expiration_date: Some("2028-04-13T00:00:00Z".into()),
        };
        let s = serde_json::to_string(&req).unwrap();
        let back: IssueCredentialRequest = serde_json::from_str(&s).unwrap();
        assert_eq!(back.credential_type, req.credential_type);
        assert_eq!(back.subject.as_str(), "did:key:zSubject");
        assert_eq!(back.evidence_refs, req.evidence_refs);
    }

    #[test]
    fn issue_credential_request_credential_type_is_pascal_case_on_wire() {
        // The frontend sends `"credential_type": "FormalCredential"` to
        // match §7's JSON-LD type name. If serde re-cased to "formalCredential"
        // every inbound IPC call would fail.
        let req = IssueCredentialRequest {
            credential_type: CredentialType::FormalCredential,
            subject: Did("did:key:z".into()),
            claim: Claim::Skill(SkillClaim {
                skill_id: "s".into(),
                level: 1,
                score: 0.1,
                evidence_refs: vec![],
                rubric_version: None,
                assessment_method: None,
            }),
            evidence_refs: vec![],
            expiration_date: None,
        };
        let v = serde_json::to_value(&req).unwrap();
        assert_eq!(
            v.get("credential_type").and_then(|x| x.as_str()),
            Some("FormalCredential")
        );
    }
}
