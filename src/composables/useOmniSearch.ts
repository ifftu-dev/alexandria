import { ref, computed, readonly } from 'vue'
import { useLocalApi } from '@/composables/useLocalApi'
import type { Course, CatalogEntry, SkillInfo, DaoInfo, Classroom } from '@/types'

/**
 * A single result surfaced by the omni search palette.
 */
export interface OmniSearchResult {
  id: string
  type: 'skill' | 'course' | 'catalog' | 'dao' | 'classroom'
  title: string
  subtitle?: string
  icon?: string
  route: string
}

const GROUP_ORDER: OmniSearchResult['type'][] = [
  'skill',
  'course',
  'catalog',
  'dao',
  'classroom',
]

const GROUP_LABELS: Record<OmniSearchResult['type'], string> = {
  skill: 'Skills',
  course: 'Courses',
  catalog: 'Public catalog',
  dao: 'Governance',
  classroom: 'Classrooms',
}

const PER_DOMAIN_LIMIT = 5
const DEBOUNCE_MS = 150
const RECENT_KEY = 'alexandria:omni-search-recents'
const MAX_RECENTS = 8

// ── Singleton state ────────────────────────────────────────────────

const isOpen = ref(false)
const query = ref('')
const results = ref<OmniSearchResult[]>([])
const loading = ref(false)
const selectedIndex = ref(0)
const recents = ref<OmniSearchResult[]>(loadRecents())

let debounceTimer: ReturnType<typeof setTimeout> | null = null
let activeQueryToken = 0

// ── Public composable ──────────────────────────────────────────────

export function useOmniSearch() {
  const { invoke } = useLocalApi()

  /** Visible items — either search results or recent items when empty. */
  const visibleItems = computed<OmniSearchResult[]>(() => {
    if (query.value.trim().length > 0) return results.value
    return recents.value
  })

  /** Items grouped by type, in the canonical order. */
  const groupedItems = computed(() => {
    const items = visibleItems.value
    const groups: { type: OmniSearchResult['type']; label: string; items: OmniSearchResult[] }[] = []
    for (const type of GROUP_ORDER) {
      const items_for_type = items.filter(i => i.type === type)
      if (items_for_type.length > 0) {
        groups.push({ type, label: GROUP_LABELS[type], items: items_for_type })
      }
    }
    return groups
  })

  function open() {
    isOpen.value = true
    selectedIndex.value = 0
  }

  function close() {
    isOpen.value = false
    query.value = ''
    results.value = []
    selectedIndex.value = 0
    if (debounceTimer) {
      clearTimeout(debounceTimer)
      debounceTimer = null
    }
  }

  function setQuery(q: string) {
    query.value = q
    selectedIndex.value = 0
    if (debounceTimer) clearTimeout(debounceTimer)
    const trimmed = q.trim()
    if (!trimmed) {
      results.value = []
      loading.value = false
      return
    }
    debounceTimer = setTimeout(() => {
      void runQuery(trimmed)
    }, DEBOUNCE_MS)
  }

  /** Move the selected index by +1 or -1 with wrap-around. */
  function navigate(direction: 1 | -1) {
    const total = visibleItems.value.length
    if (total === 0) return
    const next = (selectedIndex.value + direction + total) % total
    selectedIndex.value = next
  }

  /** Select the currently highlighted item — returns the route or null. */
  function select(): string | null {
    const item = visibleItems.value[selectedIndex.value]
    if (!item) return null
    addRecent(item)
    close()
    return item.route
  }

  /** Directly select a result by index (used on click). */
  function selectAt(index: number): string | null {
    const item = visibleItems.value[index]
    if (!item) return null
    addRecent(item)
    close()
    return item.route
  }

  async function runQuery(q: string) {
    const token = ++activeQueryToken
    loading.value = true
    try {
      const [skills, courses, catalog, daos, classrooms] = await Promise.all([
        invoke<SkillInfo[]>('list_skills', { search: q }).catch(() => []),
        invoke<Course[]>('list_courses').catch(() => []),
        invoke<CatalogEntry[]>('search_catalog', { query: q, limit: PER_DOMAIN_LIMIT }).catch(() => []),
        invoke<DaoInfo[]>('list_daos', { search: q }).catch(() => []),
        invoke<Classroom[]>('classroom_list').catch(() => []),
      ])

      if (token !== activeQueryToken) return // a newer query superseded this

      const lower = q.toLowerCase()
      const merged: OmniSearchResult[] = [
        ...skills.slice(0, PER_DOMAIN_LIMIT).map(skillToResult),
        ...courses
          .filter(c => matchesCourse(c, lower))
          .slice(0, PER_DOMAIN_LIMIT)
          .map(courseToResult),
        ...catalog.slice(0, PER_DOMAIN_LIMIT).map(catalogToResult),
        ...daos.slice(0, PER_DOMAIN_LIMIT).map(daoToResult),
        ...classrooms
          .filter(c => matchesClassroom(c, lower))
          .slice(0, PER_DOMAIN_LIMIT)
          .map(classroomToResult),
      ]

      results.value = merged
      selectedIndex.value = 0
    } finally {
      if (token === activeQueryToken) loading.value = false
    }
  }

  return {
    isOpen: readonly(isOpen),
    query: readonly(query),
    results: readonly(results),
    loading: readonly(loading),
    selectedIndex: readonly(selectedIndex),
    visibleItems,
    groupedItems,
    recents: readonly(recents),
    open,
    close,
    setQuery,
    navigate,
    select,
    selectAt,
  }
}

// ── Mappers ────────────────────────────────────────────────────────

function skillToResult(s: SkillInfo): OmniSearchResult {
  const parts: string[] = []
  if (s.bloom_level) parts.push(capitalize(s.bloom_level))
  if (s.subject_name) parts.push(s.subject_name)
  return {
    id: `skill:${s.id}`,
    type: 'skill',
    title: s.name,
    subtitle: parts.join(' · ') || undefined,
    route: `/skills/${s.id}`,
  }
}

function courseToResult(c: Course): OmniSearchResult {
  return {
    id: `course:${c.id}`,
    type: 'course',
    title: c.title,
    subtitle: c.author_name || c.description || undefined,
    route: `/courses/${c.id}`,
  }
}

function catalogToResult(c: CatalogEntry): OmniSearchResult {
  return {
    id: `catalog:${c.course_id}`,
    type: 'catalog',
    title: c.title,
    subtitle: c.description || undefined,
    route: `/courses/${c.course_id}`,
  }
}

function daoToResult(d: DaoInfo): OmniSearchResult {
  const scopeLabel = d.scope_type === 'subject_field' ? 'Field' : 'Subject'
  return {
    id: `dao:${d.id}`,
    type: 'dao',
    title: d.name,
    subtitle: scopeLabel,
    icon: d.icon_emoji || undefined,
    route: `/governance/${d.id}`,
  }
}

function classroomToResult(c: Classroom): OmniSearchResult {
  return {
    id: `classroom:${c.id}`,
    type: 'classroom',
    title: c.name,
    subtitle: c.description || undefined,
    icon: c.icon_emoji || undefined,
    route: `/classrooms/${c.id}`,
  }
}

// ── Client-side filters (for domains without backend search) ───────

function matchesCourse(c: Course, lower: string): boolean {
  if (c.title.toLowerCase().includes(lower)) return true
  if (c.description?.toLowerCase().includes(lower)) return true
  if (c.author_name?.toLowerCase().includes(lower)) return true
  if (c.tags?.some(t => t.toLowerCase().includes(lower))) return true
  return false
}

function matchesClassroom(c: Classroom, lower: string): boolean {
  if (c.name.toLowerCase().includes(lower)) return true
  if (c.description?.toLowerCase().includes(lower)) return true
  return false
}

// ── Recents (localStorage) ─────────────────────────────────────────

function loadRecents(): OmniSearchResult[] {
  if (typeof window === 'undefined') return []
  try {
    const raw = window.localStorage.getItem(RECENT_KEY)
    if (!raw) return []
    const parsed = JSON.parse(raw) as OmniSearchResult[]
    return Array.isArray(parsed) ? parsed.slice(0, MAX_RECENTS) : []
  } catch {
    return []
  }
}

function saveRecents(items: OmniSearchResult[]) {
  if (typeof window === 'undefined') return
  try {
    window.localStorage.setItem(RECENT_KEY, JSON.stringify(items))
  } catch {
    // storage may be disabled in some contexts — silently ignore
  }
}

function addRecent(item: OmniSearchResult) {
  const existing = recents.value.filter(r => r.id !== item.id)
  const next = [item, ...existing].slice(0, MAX_RECENTS)
  recents.value = next
  saveRecents(next)
}

// ── Utils ──────────────────────────────────────────────────────────

function capitalize(s: string): string {
  return s.length === 0 ? s : s.charAt(0).toUpperCase() + s.slice(1)
}
