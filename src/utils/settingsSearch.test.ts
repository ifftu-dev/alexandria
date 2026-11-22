import { describe, expect, it } from 'vitest'
import { matchesAllTerms, searchPlugins, searchTerms } from './settingsSearch'
import type { PluginSettingsIndexEntry } from '@/types'

function first<T>(items: T[]): T {
  const item = items[0]
  if (item === undefined) throw new Error('expected at least one result')
  return item
}

const index: PluginSettingsIndexEntry[] = [
  {
    plugin_cid: 'cid-lab',
    name: 'Language Lab',
    description: 'Speak and listen in English.',
    icon_path: 'icon.svg',
    subject_tags: ['english'],
    enabled: true,
    settings: [
      { key: 'ui_language', label: 'Instruction language', description: 'Language used for instructions.', keywords: ['hindi'] },
      { key: 'speech_rate', label: 'Listening speed', description: null, keywords: [] },
      { key: 'show_hints', label: 'Show Hindi hints', description: null, keywords: [] },
      { key: 'pronunciation_packs', label: 'Pronunciation language packs', description: null, keywords: ['pronunciation'] },
    ],
  },
  {
    plugin_cid: 'cid-mcq',
    name: 'Multiple choice',
    description: 'Pick the right answer.',
    icon_path: null,
    subject_tags: [],
    enabled: false,
    settings: [],
  },
]

describe('searchTerms / matchesAllTerms', () => {
  it('splits and lowercases, empty for blank', () => {
    expect(searchTerms('  Touch   ID ')).toEqual(['touch', 'id'])
    expect(searchTerms('   ')).toEqual([])
  })

  it('requires every term somewhere in the fields', () => {
    expect(matchesAllTerms(['Security', null, 'touch id'], ['touch', 'sec'])).toBe(true)
    expect(matchesAllTerms(['Security'], ['touch'])).toBe(false)
  })
})

describe('searchPlugins', () => {
  it('returns nothing for a blank query', () => {
    expect(searchPlugins(index, '  ')).toEqual([])
  })

  it('matches a plugin by name without targeting a setting', () => {
    const results = searchPlugins(index, 'multiple')
    expect(results).toHaveLength(1)
    const hit = first(results)
    expect(hit.plugin_cid).toBe('cid-mcq')
    expect(hit.matched_settings).toEqual([])
    expect(hit.target_setting).toBeNull()
  })

  it('matches on tags and description', () => {
    expect(searchPlugins(index, 'english').map((r) => r.plugin_cid)).toEqual(['cid-lab'])
    expect(searchPlugins(index, 'right answer').map((r) => r.plugin_cid)).toEqual(['cid-mcq'])
  })

  it('reports matched settings and targets the first when only settings matched', () => {
    const hit = first(searchPlugins(index, 'hindi'))
    expect(hit.matched_settings.map((s) => s.key)).toEqual(['ui_language', 'show_hints'])
    expect(hit.target_setting).toBe('ui_language')
  })

  it('matches setting keywords', () => {
    const hit = first(searchPlugins(index, 'pronunciation'))
    expect(hit.matched_settings).toEqual([{ key: 'pronunciation_packs', label: 'Pronunciation language packs' }])
    expect(hit.target_setting).toBe('pronunciation_packs')
  })

  it('combines plugin and setting terms but does not target when the plugin matched on its own', () => {
    const hit = first(searchPlugins(index, 'language lab speed'))
    expect(hit.matched_settings.map((s) => s.key)).toEqual(['speech_rate'])
    expect(hit.target_setting).toBe('speech_rate')

    const byName = first(searchPlugins(index, 'language lab'))
    // "language" also occurs in two settings, but the plugin itself matched.
    expect(byName.matched_settings.map((s) => s.key)).toEqual(['ui_language', 'pronunciation_packs'])
    expect(byName.target_setting).toBeNull()
  })

  it('needs every term somewhere in the plugin', () => {
    expect(searchPlugins(index, 'hindi answer')).toEqual([])
  })

  it('caps the matched settings', () => {
    const hit = first(searchPlugins(index, 'l', 2))
    expect(hit.matched_settings).toHaveLength(2)
  })
})
