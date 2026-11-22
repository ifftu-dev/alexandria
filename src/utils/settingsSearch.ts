/**
 * Matching for the global Settings search (`pages/Settings.vue`).
 *
 * A query is split on whitespace into lowercase terms; a target matches when
 * every term appears somewhere in its text. Static sections and installed
 * plugins share these rules.
 */
import type { PluginSettingsIndexEntry } from '@/types'

/** Lowercase whitespace-separated terms of a query; empty for a blank query. */
export function searchTerms(query: string): string[] {
  const q = query.trim().toLowerCase()
  return q ? q.split(/\s+/) : []
}

/** Whether every term occurs in the given text fields. */
export function matchesAllTerms(fields: readonly (string | null | undefined)[], terms: readonly string[]): boolean {
  const hay = fields.filter(Boolean).join(' ').toLowerCase()
  return terms.every((term) => hay.includes(term))
}

function matchesAnyTerm(fields: readonly (string | null | undefined)[], terms: readonly string[]): boolean {
  const hay = fields.filter(Boolean).join(' ').toLowerCase()
  return terms.some((term) => hay.includes(term))
}

export interface PluginSearchMatch {
  key: string
  label: string
}

export interface PluginSearchResult {
  plugin_cid: string
  name: string
  description: string | null
  icon_path: string | null
  /** Settings whose own text matched, in manifest order, capped. */
  matched_settings: PluginSearchMatch[]
  /** Setting to scroll to when the result itself is opened: set when the
   *  plugin matched only through its settings, so opening it lands on why. */
  target_setting: string | null
}

/**
 * Match installed plugins against a query. A plugin matches when every term
 * occurs in its name, description, tags or any of its settings' labels,
 * descriptions and keywords. A setting is reported as matched when its own
 * text holds at least one term and every term occurs in that setting or the
 * plugin's own fields (so "language lab hindi" still points at the setting
 * keyed "hindi").
 */
export function searchPlugins(
  index: readonly PluginSettingsIndexEntry[],
  query: string,
  maxSettings = 3,
): PluginSearchResult[] {
  const terms = searchTerms(query)
  if (terms.length === 0) return []
  const results: PluginSearchResult[] = []
  for (const plugin of index) {
    const pluginFields = [plugin.name, plugin.description, ...plugin.subject_tags]
    const settings = plugin.settings.map((s) => ({ key: s.key, label: s.label, fields: [s.label, s.description, ...s.keywords] }))
    if (!matchesAllTerms([...pluginFields, ...settings.flatMap((s) => s.fields)], terms)) continue
    const matched: PluginSearchMatch[] = settings
      .filter((s) => matchesAnyTerm(s.fields, terms) && matchesAllTerms([...s.fields, ...pluginFields], terms))
      .map(({ key, label }) => ({ key, label }))
    const pluginOnly = matchesAllTerms(pluginFields, terms)
    results.push({
      plugin_cid: plugin.plugin_cid,
      name: plugin.name,
      description: plugin.description,
      icon_path: plugin.icon_path,
      matched_settings: matched.slice(0, maxSettings),
      target_setting: pluginOnly ? null : matched[0]?.key ?? null,
    })
  }
  return results
}
