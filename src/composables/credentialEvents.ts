/**
 * Fired after this app changes the learner's own credentials (an assessment,
 * a graded plugin, course completion, a skill bootstrap, a role assessment or
 * a direct issue minted one, an import stored some, or one was revoked),
 * so views that cache derived state, such as the skill atlas, can recount
 * without a reload. The atlas does not refresh on route changes, so every
 * issuing path must announce.
 */
export const CREDENTIALS_CHANGED = 'alexandria:credentials-changed'

export function announceCredentialsChanged(): void {
  window.dispatchEvent(new Event(CREDENTIALS_CHANGED))
}
