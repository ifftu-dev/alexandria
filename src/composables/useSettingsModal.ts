/**
 * Settings section identifiers.
 *
 * Settings is a full page now (`pages/Settings.vue`, route
 * `/settings/:section?`) — not a modal. This module survives only to
 * export the shared section-id union used by the page and its panels.
 */
export type SettingsSectionId =
  | 'account'
  | 'security'
  | 'personalization'
  | 'system'
  | 'plugins'
  | 'advanced'
