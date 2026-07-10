//! Inbound handler for ratified community-content version documents received
//! on `/alexandria/goal-templates/1.0` and `/alexandria/question-banks/1.0`.
//! Both carry a signed [`VersionDoc`] whose `kind` self-identifies the content
//! type, so one handler serves both topics: it applies the doc into the local
//! tables. The envelope was already signature- + registry-validated by the
//! network layer (these are privileged topics), so a message reaching here
//! came from an authorised publisher.

use crate::db::Database;
use crate::domain::content_ratification::{apply_version_doc, VersionDoc};
use crate::p2p::types::SignedGossipMessage;

/// Apply a received ratified goal-template / question-bank version document.
pub fn handle_content_version_message(
    db: &Database,
    message: &SignedGossipMessage,
) -> Result<usize, String> {
    let doc: VersionDoc = serde_json::from_slice(&message.payload)
        .map_err(|e| format!("invalid content version doc: {e}"))?;
    apply_version_doc(db.conn(), &doc)
}
