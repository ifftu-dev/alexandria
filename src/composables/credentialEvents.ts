/**
 * Fired after this app changes the learner's own credentials (an assessment
 * or course completion issued one, an import stored some, or one was revoked),
 * so views that cache derived state, such as the skill atlas, can recount
 * without a reload. The atlas does not refresh on route changes, so every
 * issuing path must announce.
 */
export const CREDENTIALS_CHANGED = 'alexandria:credentials-changed'

export function announceCredentialsChanged(): void {
  window.dispatchEvent(new Event(CREDENTIALS_CHANGED))
}
