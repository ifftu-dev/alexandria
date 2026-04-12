/**
 * Tests for the omni search composable.
 *
 * The composable owns module-scoped refs (singleton state). To isolate
 * tests, we reset the module cache before each test and re-import via
 * dynamic `import()`. The Tauri `invoke` bridge is mocked at module
 * level so tests never hit the real IPC boundary.
 */
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { nextTick } from 'vue'

// ── Mock Tauri IPC ─────────────────────────────────────────────────

// `invoke` return value per command is controlled by `mockResults`,
// keyed by command name. Tests set this before triggering a query.
const mockResults: Record<string, unknown> = {}

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(async (cmd: string) => {
    if (cmd in mockResults) return mockResults[cmd]
    return []
  }),
}))

// ── Test helpers ───────────────────────────────────────────────────

type UseOmniSearch = typeof import('@/composables/useOmniSearch').useOmniSearch

/** Wait longer than the composable's internal debounce (150ms). */
async function waitForDebounce() {
  await new Promise(resolve => setTimeout(resolve, 200))
  await nextTick()
}

/**
 * Load a FRESH module instance so singleton state (isOpen, recents,
 * etc.) is clean per-test.
 */
async function freshUseOmniSearch(): Promise<UseOmniSearch> {
  vi.resetModules()
  const mod = await import('@/composables/useOmniSearch')
  return mod.useOmniSearch
}

beforeEach(() => {
  window.localStorage.removeItem('alexandria:omni-search-recents')
  for (const key of Object.keys(mockResults)) delete mockResults[key]
})

// ── Tests ──────────────────────────────────────────────────────────

describe('useOmniSearch', () => {
  it('starts closed, empty', async () => {
    const s = (await freshUseOmniSearch())()
    expect(s.isOpen.value).toBe(false)
    expect(s.query.value).toBe('')
    expect(s.visibleItems.value).toEqual([])
    expect(s.selectedIndex.value).toBe(0)
  })

  it('open() / close() toggle isOpen and reset state on close', async () => {
    const s = (await freshUseOmniSearch())()
    s.open()
    expect(s.isOpen.value).toBe(true)

    s.setQuery('foo')
    expect(s.query.value).toBe('foo')

    s.close()
    expect(s.isOpen.value).toBe(false)
    expect(s.query.value).toBe('')
    expect(s.selectedIndex.value).toBe(0)
  })

  it('empty query shows recents (empty by default)', async () => {
    const s = (await freshUseOmniSearch())()
    s.open()
    expect(s.visibleItems.value).toEqual([])
  })

  it('debounces queries and merges results across domains', async () => {
    // Arrange backend responses per command
    mockResults['list_skills'] = [
      {
        id: 'sk1',
        name: 'Graph Algorithms',
        description: null,
        subject_id: 'sub1',
        subject_name: 'Algorithms',
        subject_field_id: 'fld1',
        subject_field_name: 'CS',
        bloom_level: 'analyze',
        prerequisite_count: 2,
        dependent_count: 1,
        created_at: null,
      },
    ]
    mockResults['list_courses'] = [
      {
        id: 'c1',
        title: 'Graphs 101',
        description: null,
        author_address: 'addr_test1...',
        author_name: 'Dr. Vasquez',
        content_cid: null,
        thumbnail_cid: null,
        thumbnail_svg: null,
        tags: null,
        skill_ids: null,
        version: 1,
        status: 'published',
        published_at: null,
        on_chain_tx: null,
        created_at: '2026-01-01',
        updated_at: '2026-01-01',
      },
    ]
    mockResults['search_catalog'] = []
    mockResults['list_daos'] = [
      {
        id: 'dao1',
        name: 'Computer Science',
        description: null,
        icon_emoji: '💻',
        scope_type: 'subject_field',
        scope_id: 'fld1',
        status: 'active',
        committee_size: 5,
        election_interval_days: 365,
        on_chain_tx: null,
        created_at: '',
        updated_at: '',
      },
    ]
    mockResults['classroom_list'] = []

    const s = (await freshUseOmniSearch())()
    s.open()
    s.setQuery('graph')

    // Before debounce: query is set but results not populated yet
    expect(s.query.value).toBe('graph')
    expect(s.visibleItems.value).toEqual([])

    await waitForDebounce()

    const items = s.visibleItems.value
    expect(items.length).toBeGreaterThan(0)
    expect(items.some(i => i.type === 'skill' && i.title === 'Graph Algorithms')).toBe(true)
    expect(items.some(i => i.type === 'course' && i.title === 'Graphs 101')).toBe(true)
    expect(items.some(i => i.type === 'dao' && i.title === 'Computer Science')).toBe(true)
  })

  it('filters `list_courses` client-side (backend has no search param)', async () => {
    mockResults['list_courses'] = [
      { id: 'a', title: 'Algorithms', description: null, author_address: 'x', author_name: null, content_cid: null, thumbnail_cid: null, thumbnail_svg: null, tags: null, skill_ids: null, version: 1, status: 'published', published_at: null, on_chain_tx: null, created_at: '', updated_at: '' },
      { id: 'b', title: 'UX Design', description: null, author_address: 'x', author_name: null, content_cid: null, thumbnail_cid: null, thumbnail_svg: null, tags: null, skill_ids: null, version: 1, status: 'published', published_at: null, on_chain_tx: null, created_at: '', updated_at: '' },
    ]

    const s = (await freshUseOmniSearch())()
    s.open()
    s.setQuery('algorithms')
    await waitForDebounce()

    const courses = s.visibleItems.value.filter(i => i.type === 'course')
    expect(courses).toHaveLength(1)
    expect(courses[0]!.title).toBe('Algorithms')
  })

  it('groups results by canonical domain order', async () => {
    mockResults['list_skills'] = [
      { id: 's1', name: 'Skill', description: null, subject_id: null, subject_name: null, subject_field_id: null, subject_field_name: null, bloom_level: 'apply', prerequisite_count: 0, dependent_count: 0, created_at: null },
    ]
    mockResults['list_courses'] = [
      { id: 'c1', title: 'Course match', description: null, author_address: 'x', author_name: null, content_cid: null, thumbnail_cid: null, thumbnail_svg: null, tags: null, skill_ids: null, version: 1, status: 'published', published_at: null, on_chain_tx: null, created_at: '', updated_at: '' },
    ]
    mockResults['list_daos'] = [
      { id: 'd1', name: 'DAO match', description: null, icon_emoji: null, scope_type: 'subject', scope_id: '', status: 'active', committee_size: 5, election_interval_days: 365, on_chain_tx: null, created_at: '', updated_at: '' },
    ]

    const s = (await freshUseOmniSearch())()
    s.open()
    s.setQuery('match')
    await waitForDebounce()

    const groups = s.groupedItems.value
    const types = groups.map(g => g.type)
    // Skills must come before courses must come before daos
    const iSkill = types.indexOf('skill')
    const iCourse = types.indexOf('course')
    const iDao = types.indexOf('dao')
    expect(iSkill).toBeGreaterThanOrEqual(0)
    expect(iSkill).toBeLessThan(iCourse)
    expect(iCourse).toBeLessThan(iDao)
  })

  it('navigate() wraps around with up/down', async () => {
    mockResults['list_skills'] = [
      { id: 's1', name: 'A', description: null, subject_id: null, subject_name: null, subject_field_id: null, subject_field_name: null, bloom_level: 'apply', prerequisite_count: 0, dependent_count: 0, created_at: null },
      { id: 's2', name: 'B', description: null, subject_id: null, subject_name: null, subject_field_id: null, subject_field_name: null, bloom_level: 'apply', prerequisite_count: 0, dependent_count: 0, created_at: null },
      { id: 's3', name: 'C', description: null, subject_id: null, subject_name: null, subject_field_id: null, subject_field_name: null, bloom_level: 'apply', prerequisite_count: 0, dependent_count: 0, created_at: null },
    ]

    const s = (await freshUseOmniSearch())()
    s.open()
    s.setQuery('x')
    await waitForDebounce()

    expect(s.visibleItems.value.length).toBe(3)
    expect(s.selectedIndex.value).toBe(0)

    s.navigate(1)
    expect(s.selectedIndex.value).toBe(1)
    s.navigate(1)
    expect(s.selectedIndex.value).toBe(2)
    s.navigate(1)
    expect(s.selectedIndex.value).toBe(0) // wrap forward
    s.navigate(-1)
    expect(s.selectedIndex.value).toBe(2) // wrap backward
  })

  it('select() returns the current item route and persists it to recents', async () => {
    mockResults['list_skills'] = [
      {
        id: 'sk-graph',
        name: 'Graph Theory',
        description: null,
        subject_id: 'dm',
        subject_name: 'Discrete Math',
        subject_field_id: 'math',
        subject_field_name: 'Math',
        bloom_level: 'analyze',
        prerequisite_count: 0,
        dependent_count: 0,
        created_at: null,
      },
    ]

    const s = (await freshUseOmniSearch())()
    s.open()
    s.setQuery('graph')
    await waitForDebounce()

    const route = s.select()
    expect(route).toBe('/skills/sk-graph')
    // Palette closes after select
    expect(s.isOpen.value).toBe(false)

    // Recents is now populated
    const raw = window.localStorage.getItem('alexandria:omni-search-recents')
    expect(raw).not.toBeNull()
    const recents = JSON.parse(raw!) as Array<{ id: string; route: string }>
    expect(recents).toHaveLength(1)
    expect(recents[0]!.route).toBe('/skills/sk-graph')
  })

  it('setQuery("") clears results and cancels pending fetch', async () => {
    mockResults['list_skills'] = [
      { id: 's1', name: 'A', description: null, subject_id: null, subject_name: null, subject_field_id: null, subject_field_name: null, bloom_level: 'apply', prerequisite_count: 0, dependent_count: 0, created_at: null },
    ]

    const s = (await freshUseOmniSearch())()
    s.open()
    s.setQuery('a')
    s.setQuery('') // immediately clear

    await waitForDebounce()

    expect(s.query.value).toBe('')
    // Only recents would show here; recents are empty in this test
    expect(s.visibleItems.value).toEqual([])
  })
})
