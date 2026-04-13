//! IPC commands for selective disclosure + presentations. Stubs — PR 11.

use tauri::State;

use crate::AppState;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CreatePresentationRequest {
    pub credential_ids: Vec<String>,
    /// JSONPath-style field subsets to reveal (e.g. ["credentialSubject.claim.level"]).
    pub reveal: Vec<String>,
    pub audience: String,
    pub nonce: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PresentationEnvelope {
    pub id: String,
    pub payload_json: String,
    pub proof: String,
}

#[tauri::command]
pub async fn create_presentation(
    _state: State<'_, AppState>,
    _req: CreatePresentationRequest,
) -> Result<PresentationEnvelope, String> {
    Err("PR 11 — create_presentation not yet implemented".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_presentation_request_round_trips() {
        let req = CreatePresentationRequest {
            credential_ids: vec!["urn:uuid:c1".into(), "urn:uuid:c2".into()],
            reveal: vec!["credentialSubject.claim.level".into()],
            audience: "did:web:hirer.example".into(),
            nonce: "nonce-1234".into(),
        };
        let s = serde_json::to_string(&req).unwrap();
        let back: CreatePresentationRequest = serde_json::from_str(&s).unwrap();
        assert_eq!(back.credential_ids, req.credential_ids);
        assert_eq!(back.reveal, req.reveal);
        assert_eq!(back.audience, req.audience);
        assert_eq!(back.nonce, req.nonce);
    }

    #[test]
    fn presentation_envelope_round_trips() {
        let env = PresentationEnvelope {
            id: "pres-1".into(),
            payload_json: "{\"sub\":\"did:key:z\"}".into(),
            proof: "sig".into(),
        };
        let s = serde_json::to_string(&env).unwrap();
        let back: PresentationEnvelope = serde_json::from_str(&s).unwrap();
        assert_eq!(back.id, env.id);
        assert_eq!(back.payload_json, env.payload_json);
        assert_eq!(back.proof, env.proof);
    }
}
