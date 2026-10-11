/**
 * Fired after this app changes the learner's own credentials (an assessment
 * issued one, or one was revoked), so views that cache derived state, such as
 * the sidebar skill graph, can recount without a reload.
 */
export const CREDENTIALS_CHANGED = 'alexandria:credentials-changed'

export function announceCredentialsChanged(): void {
  window.dispatchEvent(new Event(CREDENTIALS_CHANGED))
}
