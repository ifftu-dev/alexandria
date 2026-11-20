//! Refusal codes in plain words.
//!
//! When a closed hosted attempt issues nothing, the service records the first
//! unmet requirement and the candidate's app reads its code: one of the shared
//! issuance rule's ([`alexandria_verify::policy::PolicyRefusal`]) with that
//! rule's fields, or one of the service's own (the evidence did not verify,
//! the organisation's domain does not list its key, issuance failed). The
//! candidate is told what was missing and whether another attempt could meet
//! it, so they know whether to retake (story 20).

use serde::Serialize;
use serde_json::Value;

/// A refusal as the candidate reads it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Explanation {
    /// The stable code, `unknown` when the refusal names none.
    pub code: String,
    /// What was missing, in plain words.
    pub message: String,
    /// Another attempt could meet the requirement (within the retake rule);
    /// false when the cause is the organisation's or the role's, not the
    /// attempt's.
    pub retake_may_help: bool,
}

fn rung_words(rung: &str) -> &str {
    match rung {
        "none" => "no verification",
        "process" => "process checks only",
        "probed" => "a follow-up question you had not seen, answered rightly in time",
        "consistent" => "consistent answers across follow-up questions",
        "live" => "a live interview with the organisation",
        other => other,
    }
}

fn percent(value: Option<f64>) -> String {
    value
        .map(|v| format!("{:.0}%", v * 100.0))
        .unwrap_or_else(|| "?".into())
}

/// Explain the refusal the service recorded: `{"code": ..., ...fields}`.
pub fn explain(refusal: &Value) -> Explanation {
    let code = refusal
        .get("code")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    let text = |key: &str| refusal.get(key).and_then(Value::as_str).unwrap_or_default();
    let number = |key: &str| refusal.get(key).and_then(Value::as_f64);
    let count = |key: &str| refusal.get(key).and_then(Value::as_i64).unwrap_or_default();
    let (message, retake_may_help) = match code.as_str() {
        "custody_below_required" => (
            "This role only issues for an attempt taken on the organisation's service, where \
             the organisation holds the answers. A credential from your own device does not \
             count."
                .to_string(),
            false,
        ),
        "not_clean" => (
            "The integrity session did not end clean: something was flagged or sent for review."
                .to_string(),
            true,
        ),
        "integrity_below_minimum" => (
            format!(
                "The integrity score was {}, below the {} the organisation requires.",
                percent(number("score")),
                percent(number("min"))
            ),
            true,
        ),
        "no_integrity_score" => (
            "The integrity session has no score, and the organisation requires a minimum."
                .to_string(),
            true,
        ),
        "too_many_critical" => (
            format!(
                "The session had {} critical flags; the organisation allows {}.",
                count("count"),
                count("max")
            ),
            true,
        ),
        "too_many_warnings" => (
            format!(
                "The session had {} warnings; the organisation allows {}.",
                count("count"),
                count("max")
            ),
            true,
        ),
        "assurance_below_required" if text("required") == "device_attested" => (
            "This role must be taken in Alexandria on a phone that proves it runs the genuine \
             app. A computer with a paired phone does not count."
                .to_string(),
            true,
        ),
        "assurance_below_required" => (
            "The device check the organisation requires was not met.".to_string(),
            true,
        ),
        "trust_profile_below_floor" => (
            format!(
                "The monitoring profile ({}) was below the {} profile the organisation requires.",
                text("profile"),
                text("required")
            ),
            true,
        ),
        "rung_below_minimum" => (
            format!(
                "The organisation requires {}; this attempt reached {}.",
                rung_words(text("required")),
                rung_words(text("rung"))
            ),
            true,
        ),
        "probe_policy_not_run" => (
            "The organisation's follow-up questions did not run in this attempt.".to_string(),
            true,
        ),
        "probe_failed" => (
            "A follow-up question was answered wrongly or after its time limit.".to_string(),
            true,
        ),
        "attestation_evidence_invalid" => (
            "This device's proof that it runs the genuine Alexandria app did not check out, so \
             the organisation could not rely on the session."
                .to_string(),
            true,
        ),
        "no_integrity_assertion" => ("The attempt carried no integrity result.".to_string(), true),
        "unverified_assurance_claim" => (
            "The session claimed a stronger device check than the organisation could verify."
                .to_string(),
            true,
        ),
        "organisation_not_verified" => (
            "The organisation's domain does not currently list its signing key, so it cannot \
             issue a credential. This is for the organisation to fix."
                .to_string(),
            false,
        ),
        "subject_is_organisation_key" => (
            "The organisation cannot issue a credential to its own key.".to_string(),
            false,
        ),
        "issuance_unavailable" | "share_not_accepted" => (
            "The organisation's service could not issue a credential. Your result is recorded."
                .to_string(),
            false,
        ),
        other => (
            format!("The organisation did not issue a credential ({other})."),
            false,
        ),
    };
    Explanation {
        code,
        message,
        retake_may_help,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_refusal_code_maps_to_its_message() {
        let rung = explain(&json!({
            "code": "rung_below_minimum", "rung": "process", "required": "probed"
        }));
        assert_eq!(rung.code, "rung_below_minimum");
        assert_eq!(
            rung.message,
            "The organisation requires a follow-up question you had not seen, answered \
             rightly in time; this attempt reached process checks only."
        );
        assert!(rung.retake_may_help);

        let late = explain(&json!({"code": "probe_failed", "policy": "authored"}));
        assert_eq!(
            late.message,
            "A follow-up question was answered wrongly or after its time limit."
        );
        assert!(late.retake_may_help);

        let device = explain(&json!({
            "code": "assurance_below_required", "level": "companion_attested",
            "required": "device_attested"
        }));
        assert_eq!(
            device.message,
            "This role must be taken in Alexandria on a phone that proves it runs the genuine \
             app. A computer with a paired phone does not count."
        );

        let evidence = explain(&json!({"code": "attestation_evidence_invalid"}));
        assert_eq!(
            evidence.message,
            "This device's proof that it runs the genuine Alexandria app did not check out, \
             so the organisation could not rely on the session."
        );

        let unverified = explain(&json!({"code": "organisation_not_verified"}));
        assert!(
            !unverified.retake_may_help,
            "the organisation must act, not the candidate"
        );
    }

    #[test]
    fn an_unknown_code_is_named_rather_than_hidden() {
        let unknown = explain(&json!({"code": "something_new"}));
        assert_eq!(unknown.code, "something_new");
        assert_eq!(
            unknown.message,
            "The organisation did not issue a credential (something_new)."
        );
        assert!(!unknown.retake_may_help);
        assert_eq!(explain(&json!("garbage")).code, "unknown");
    }
}
