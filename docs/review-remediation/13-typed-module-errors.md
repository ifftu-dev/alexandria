# Typed module errors

Triage: ready-for-agent
Repos: alexandria
Part of: alexandria/docs/review-remediation-spec.md
Blocked by: alexandria/docs/review-remediation/06-notification-domain-types-and-vocabulary.md, alexandria/docs/review-remediation/11-guardian-ai-requests-through-ai-access.md, alexandria/docs/review-remediation/12-credentials-signed-by-a-whole-issuer.md

## Task

- **Error enums.** The notifications, credentials, AI access, enrollment, aggregation and AI chat operation modules each expose a thiserror enum at their public interface. They may use anyhow internally. Commands keep returning `Result<T, String>` and convert at the boundary.
- **Studio access refusal.** Rewrite it on thiserror and rename it so the name says it is an access denial.
- **Chat turn outcome.** Rename the dropped fenced-block outcome so the name says what happened.
- **Conversion.** Remove the two-way conversion between access refusals and Studio errors. Callers map explicitly.

Covers spec stories 68–70.

## Acceptance

- Each module's tests assert refusal variants rather than error strings.
- Command error text seen by the webview is unchanged, or any change is listed.
- Clippy with `-D warnings` and the guard script pass.
