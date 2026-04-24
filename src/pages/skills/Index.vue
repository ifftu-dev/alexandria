<script setup lang="ts">
import { ref, computed, onMounted, onBeforeUnmount, watch, nextTick } from 'vue'
import { useRouter } from 'vue-router'
import { useLocalApi } from '@/composables/useLocalApi'
import { useSkillGraphHover } from '@/composables/useSkillGraphHover'
import { AppBadge, AppTabs } from '@/components/ui'
import type {
  SubjectFieldInfo,
  SubjectInfo,
  SkillInfo,
  SkillGraphEdge,
  VerifiableCredential,
} from '@/types'
import { earnedSkillIdsFromCredentials } from '@/composables/useSkillGraphState'

const { invoke } = useLocalApi()
const router = useRouter()

const loading = ref(true)
const fields = ref<SubjectFieldInfo[]>([])
const subjects = ref<SubjectInfo[]>([])
const skills = ref<SkillInfo[]>([])
const graphEdges = ref<SkillGraphEdge[]>([])
const myCredentials = ref<VerifiableCredential[]>([])
const localDid = ref<string | null>(null)

const search = ref('')
const selectedField = ref<string | null>(null)
const selectedSubject = ref<string | null>(null)

const activeTab = ref('graph')
const tabs = [
  { key: 'graph', label: 'My Graph' },
  { key: 'browse', label: 'Browse Taxonomy' },
  { key: 'credentials', label: 'My Credentials' },
]

onMounted(async () => {
  try {
    await invoke<number>('bootstrap_public_taxonomy', {}).catch(() => 0)

    const [f, s, sk, edges, did, creds] = await Promise.all([
      invoke<SubjectFieldInfo[]>('list_subject_fields', {}),
      invoke<SubjectInfo[]>('list_subjects', {}),
      invoke<SkillInfo[]>('list_skills', {}),
      invoke<SkillGraphEdge[]>('list_skill_graph_edges', {}),
      invoke<string | null>('get_local_did').catch(() => null),
      invoke<VerifiableCredential[]>('list_credentials', {}).catch(() => []),
    ])
    fields.value = f
    subjects.value = s
    skills.value = sk
    graphEdges.value = edges
    localDid.value = did
    myCredentials.value = creds
  } catch (e) {
    console.error('Failed to load taxonomy:', e)
  } finally {
    loading.value = false
  }
})

// Filter subjects when a field is selected
const filteredSubjects = computed(() => {
  if (!selectedField.value) return subjects.value
  return subjects.value.filter(s => s.subject_field_id === selectedField.value)
})

// Filter skills based on selections and search
const filteredSkills = computed(() => {
  let result = skills.value

  if (selectedSubject.value) {
    result = result.filter(sk => sk.subject_id === selectedSubject.value)
  } else if (selectedField.value) {
    result = result.filter(sk => sk.subject_field_id === selectedField.value)
  }

  if (search.value.trim()) {
    const q = search.value.toLowerCase()
    result = result.filter(
      sk =>
        sk.name.toLowerCase().includes(q) ||
        (sk.description && sk.description.toLowerCase().includes(q)) ||
        sk.bloom_level.toLowerCase().includes(q)
    )
  }

  return result
})

// Stats
const totalSkills = computed(() => skills.value.length)

function selectField(id: string | null) {
  selectedField.value = selectedField.value === id ? null : id
  selectedSubject.value = null
}

function selectSubject(id: string | null) {
  selectedSubject.value = selectedSubject.value === id ? null : id
}

function goToSkill(id: string) {
  router.push(`/skills/${id}`)
}

const bloomColors: Record<string, string> = {
  remember: 'secondary',
  understand: 'primary',
  apply: 'accent',
  analyze: 'warning',
  evaluate: 'success',
  create: 'governance',
}

const bloomOrder = ['remember', 'understand', 'apply', 'analyze', 'evaluate', 'create']

const earnedSkillIdSet = computed(() =>
  earnedSkillIdsFromCredentials(myCredentials.value, localDid.value),
)

const mySkillCredentials = computed(() =>
  myCredentials.value.filter((vc) => {
    if (localDid.value && vc.credential_subject.id !== localDid.value) return false
    return vc.credential_subject.claim.kind === 'skill'
  }),
)

const prereqMap = computed(() => {
  const map = new Map<string, string[]>()
  for (const edge of graphEdges.value) {
    if (!map.has(edge.skill_id)) map.set(edge.skill_id, [])
    map.get(edge.skill_id)!.push(edge.prerequisite_id)
  }
  return map
})

const availableSkillIdSet = computed(() => {
  const earned = earnedSkillIdSet.value
  const set = new Set<string>()
  for (const skill of skills.value) {
    if (earned.has(skill.id)) continue
    const prereqs = prereqMap.value.get(skill.id) ?? []
    if (prereqs.length === 0 || prereqs.every((id) => earned.has(id))) {
      set.add(skill.id)
    }
  }
  return set
})

const personalSkillIdSet = computed(() => {
  const include = new Set<string>()
  const earned = earnedSkillIdSet.value

  function includePrereqClosure(skillId: string, seen = new Set<string>()) {
    if (seen.has(skillId)) return
    seen.add(skillId)
    include.add(skillId)
    const prereqs = prereqMap.value.get(skillId) ?? []
    for (const p of prereqs) includePrereqClosure(p, seen)
  }

  for (const id of earned) includePrereqClosure(id)
  for (const id of availableSkillIdSet.value) include.add(id)
  return include
})

const earnedSkillsCount = computed(() => earnedSkillIdSet.value.size)
const availableSkillsCount = computed(() => availableSkillIdSet.value.size)
const lockedSkillsCount = computed(() =>
  Math.max(0, skills.value.length - earnedSkillsCount.value - availableSkillsCount.value)
)

// ============ Force-graph (same renderer as sidebar/modal) ============
const graphContainerRef = ref<HTMLElement | null>(null)
// eslint-disable-next-line @typescript-eslint/no-explicit-any
const forceGraphInstance = ref<any>(null)
let graphResizeObserver: ResizeObserver | null = null
const { buildAdjacency, createHoverHandler, renderNode, renderLink } = useSkillGraphHover()

const forceGraphNodes = computed(() => {
  const earned = earnedSkillIdSet.value
  const ids = personalSkillIdSet.value
  return skills.value
    .filter(s => ids.has(s.id))
    .map(skill => {
      const prereqs = prereqMap.value.get(skill.id) ?? []
      const status = earned.has(skill.id)
        ? 'earned'
        : (prereqs.length === 0 || prereqs.every(p => earned.has(p)))
            ? 'available'
            : 'locked'
      return { id: skill.id, name: skill.name, routeId: skill.id, status, prerequisites: prereqs }
    })
})

function destroyForceGraph() {
  graphResizeObserver?.disconnect()
  graphResizeObserver = null
  if (forceGraphInstance.value) {
    forceGraphInstance.value._destructor?.()
    forceGraphInstance.value = null
  }
}

async function initForceGraph() {
  if (!graphContainerRef.value || !forceGraphNodes.value.length) return
  destroyForceGraph()

  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const ForceGraph = (await import('force-graph')).default as any

  const links: Array<{ source: string; target: string }> = []
  for (const node of forceGraphNodes.value) {
    for (const prereqId of node.prerequisites) {
      if (personalSkillIdSet.value.has(prereqId)) {
        links.push({ source: prereqId, target: node.id })
      }
    }
  }
  buildAdjacency(links)

  const width = graphContainerRef.value.clientWidth
  const height = graphContainerRef.value.clientHeight

  const graph = ForceGraph()(graphContainerRef.value)
    .width(width)
    .height(height)
    .graphData({ nodes: forceGraphNodes.value, links })
    .autoPauseRedraw(false)
    .nodeLabel(() => '')
    .onNodeHover(createHoverHandler())
    .nodeCanvasObject((node: Record<string, unknown>, ctx: CanvasRenderingContext2D, globalScale: number) => {
      renderNode(node, ctx, globalScale)
    })
    .linkCanvasObject((link: Record<string, unknown>, ctx: CanvasRenderingContext2D) => {
      renderLink(link, ctx)
    })
    .linkCanvasObjectMode(() => 'replace' as const)
    .backgroundColor('transparent')
    .onNodeClick((node: Record<string, unknown>) => {
      const routeId = String(node.routeId ?? node.id ?? '')
      if (routeId) router.push(`/skills/${routeId}`)
    })
    .cooldownTicks(100)
    .onEngineStop(() => {
      graph.zoomToFit(400, 40)
    })

  graph.d3Force('charge')?.strength(-80)
  graph.d3Force('link')?.distance(50)

  forceGraphInstance.value = graph

  graphResizeObserver = new ResizeObserver((entries) => {
    for (const entry of entries) {
      graph.width(entry.contentRect.width)
      graph.height(entry.contentRect.height)
    }
  })
  graphResizeObserver.observe(graphContainerRef.value)
}

// Init/destroy the force graph when switching to/from the graph tab
watch(activeTab, async (tab) => {
  if (tab === 'graph' && !loading.value) {
    await nextTick()
    initForceGraph()
  } else {
    destroyForceGraph()
  }
})

// Also init if data loads while graph tab is already active
watch(loading, async (isLoading) => {
  if (!isLoading && activeTab.value === 'graph') {
    await nextTick()
    initForceGraph()
  }
})

onBeforeUnmount(() => {
  destroyForceGraph()
})
</script>

<template>
  <div>
    <!-- Header -->
    <div class="mb-8">
      <h1 class="text-3xl font-bold text-foreground">My Skill Graph</h1>
      <p class="mt-2 text-muted-foreground">
        Your personal skill progression graph derived from your proofs, unlocked skills, and prerequisite chains.
      </p>
    </div>

    <!-- Skeleton -->
    <div v-if="loading" class="space-y-6">
      <div class="grid grid-cols-2 sm:grid-cols-4 gap-4">
        <div v-for="i in 4" :key="i" class="animate-pulse rounded-xl bg-card shadow-sm p-5 text-center">
          <div class="h-7 w-10 mx-auto rounded bg-muted-foreground/20 mb-2" />
          <div class="h-3 w-16 mx-auto rounded bg-muted-foreground/10" />
        </div>
      </div>
      <div class="h-10 w-full animate-pulse rounded-lg bg-muted-foreground/8" />
      <div class="flex gap-4">
        <div class="w-64 space-y-3">
          <div v-for="i in 2" :key="i" class="animate-pulse rounded-xl bg-card shadow-sm p-4">
            <div class="h-3 w-20 rounded bg-muted-foreground/15 mb-3" />
            <div v-for="j in 4" :key="j" class="h-7 w-full rounded bg-muted-foreground/8 mb-1.5" />
          </div>
        </div>
        <div class="flex-1 space-y-2">
          <div v-for="i in 5" :key="i" class="animate-pulse rounded-xl bg-card shadow-sm p-4">
            <div class="flex items-start justify-between">
              <div class="space-y-2 flex-1">
                <div class="h-4 w-48 rounded bg-muted-foreground/15" />
                <div class="h-3 w-full rounded bg-muted-foreground/8" />
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>

    <template v-else>
      <!-- Stats bar -->
      <div class="mb-6 grid grid-cols-2 sm:grid-cols-4 gap-4">
        <div class="rounded-xl bg-card shadow-sm p-5 text-center">
          <p class="font-mono text-2xl font-bold text-success">{{ earnedSkillsCount }}</p>
          <p class="text-xs text-muted-foreground flex items-center justify-center gap-1 mt-1">
            <svg class="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
              <path stroke-linecap="round" stroke-linejoin="round" d="M2.25 12.75V12A2.25 2.25 0 014.5 9.75h15A2.25 2.25 0 0121.75 12v.75m-8.69-6.44l-2.12-2.12a1.5 1.5 0 00-1.061-.44H4.5A2.25 2.25 0 002.25 6v12a2.25 2.25 0 002.25 2.25h15A2.25 2.25 0 0021.75 18V9a2.25 2.25 0 00-2.25-2.25h-5.379a1.5 1.5 0 01-1.06-.44z" />
            </svg>
            Earned
          </p>
        </div>
        <div class="rounded-xl bg-card shadow-sm p-5 text-center">
          <p class="font-mono text-2xl font-bold text-warning">{{ availableSkillsCount }}</p>
          <p class="text-xs text-muted-foreground flex items-center justify-center gap-1 mt-1">
            <svg class="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
              <path stroke-linecap="round" stroke-linejoin="round" d="M12 6.042A8.967 8.967 0 006 3.75c-1.052 0-2.062.18-3 .512v14.25A8.987 8.987 0 016 18c2.305 0 4.408.867 6 2.292m0-14.25a8.966 8.966 0 016-2.292c1.052 0 2.062.18 3 .512v14.25A8.987 8.987 0 0018 18a8.967 8.967 0 00-6 2.292m0-14.25v14.25" />
            </svg>
            Available
          </p>
        </div>
        <div class="rounded-xl bg-card shadow-sm p-5 text-center">
          <p class="font-mono text-2xl font-bold text-muted-foreground">{{ lockedSkillsCount }}</p>
          <p class="text-xs text-muted-foreground flex items-center justify-center gap-1 mt-1">
            <svg class="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
              <path stroke-linecap="round" stroke-linejoin="round" d="M9.813 15.904L9 18.75l-.813-2.846a4.5 4.5 0 00-3.09-3.09L2.25 12l2.846-.813a4.5 4.5 0 003.09-3.09L9 5.25l.813 2.846a4.5 4.5 0 003.09 3.09L15.75 12l-2.846.813a4.5 4.5 0 00-3.09 3.09zM18.259 8.715L18 9.75l-.259-1.035a3.375 3.375 0 00-2.455-2.456L14.25 6l1.036-.259a3.375 3.375 0 002.455-2.456L18 2.25l.259 1.035a3.375 3.375 0 002.455 2.456L21.75 6l-1.036.259a3.375 3.375 0 00-2.455 2.456z" />
            </svg>
            Locked
          </p>
        </div>
        <div class="rounded-xl bg-card shadow-sm p-5 text-center">
          <p class="font-mono text-2xl font-bold text-foreground">{{ totalSkills }}</p>
          <p class="text-xs text-muted-foreground flex items-center justify-center gap-1 mt-1">
            <svg class="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
              <path stroke-linecap="round" stroke-linejoin="round" d="M7.5 21L3 16.5m0 0L7.5 12M3 16.5h13.5m0-13.5L21 7.5m0 0L16.5 12M21 7.5H7.5" />
            </svg>
            Taxonomy Skills
          </p>
        </div>
      </div>

      <!-- Tabs -->
      <AppTabs :tabs="tabs" v-model="activeTab" class="mb-6" />

      <!-- ============ BROWSE TAB ============ -->
      <div v-if="activeTab === 'browse'" class="space-y-4">
        <!-- Search -->
        <input
          v-model="search"
          class="w-full rounded-lg border border-border bg-background px-4 py-2.5 text-sm text-foreground placeholder-muted-foreground/50 transition-colors focus:border-primary focus:outline-none focus:ring-1 focus:ring-primary"
          placeholder="Search skills by name, description, or level..."
        >

        <div v-if="totalSkills === 0" class="py-16 text-center">
          <div class="mx-auto mb-4 flex h-16 w-16 items-center justify-center rounded-full bg-primary/10">
            <svg class="h-8 w-8 text-primary" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
              <path stroke-linecap="round" stroke-linejoin="round" d="M9.813 15.904L9 18.75l-.813-2.846a4.5 4.5 0 00-3.09-3.09L2.25 12l2.846-.813a4.5 4.5 0 003.09-3.09L9 5.25l.813 2.846a4.5 4.5 0 003.09 3.09L15.75 12l-2.846.813a4.5 4.5 0 00-3.09 3.09z" />
            </svg>
          </div>
          <h3 class="text-lg font-semibold text-foreground">No skills in the taxonomy</h3>
          <p class="mt-1 text-sm text-muted-foreground max-w-md mx-auto">
            Skills are added through the governance taxonomy proposal workflow. Create a DAO, propose a taxonomy change, and ratify it to populate the skill graph.
          </p>
        </div>

        <div v-else class="flex flex-col md:flex-row gap-4">
          <!-- Left: Hierarchy panel — collapsible on mobile -->
          <div class="w-full md:w-64 md:flex-shrink-0 space-y-3">
            <!-- Subject Fields -->
            <div class="rounded-xl bg-card shadow-sm p-4">
              <p class="text-[10px] font-semibold text-muted-foreground mb-3 tracking-wider uppercase">Subject Fields</p>
              <button
                v-if="selectedField"
                class="w-full text-left text-xs px-2 py-1 mb-1 rounded-lg text-primary hover:bg-primary/10"
                @click="selectField(null)"
              >
                Show all
              </button>
              <div
                v-for="field in fields"
                :key="field.id"
                class="rounded-lg px-2.5 py-2 text-sm cursor-pointer transition-colors"
                :class="selectedField === field.id
                  ? 'bg-primary/10 text-primary font-medium'
                  : 'text-foreground hover:bg-muted/50'"
                @click="selectField(field.id)"
              >
                <div class="flex items-center justify-between">
                  <span class="truncate">
                    <span v-if="field.icon_emoji" class="mr-1.5">{{ field.icon_emoji }}</span>{{ field.name }}
                  </span>
                  <span class="text-xs text-muted-foreground tabular-nums">{{ field.skill_count }}</span>
                </div>
              </div>
              <p v-if="fields.length === 0" class="text-xs text-muted-foreground italic px-2">
                No subject fields
              </p>
            </div>

            <!-- Subjects -->
            <div class="rounded-xl bg-card shadow-sm p-4">
              <p class="text-[10px] font-semibold text-muted-foreground mb-3 tracking-wider uppercase">Subjects</p>
              <button
                v-if="selectedSubject"
                class="w-full text-left text-xs px-2 py-1 mb-1 rounded-lg text-primary hover:bg-primary/10"
                @click="selectSubject(null)"
              >
                Show all
              </button>
              <div
                v-for="subj in filteredSubjects"
                :key="subj.id"
                class="rounded-lg px-2.5 py-2 text-sm cursor-pointer transition-colors"
                :class="selectedSubject === subj.id
                  ? 'bg-primary/10 text-primary font-medium'
                  : 'text-foreground hover:bg-muted/50'"
                @click="selectSubject(subj.id)"
              >
                <div class="flex items-center justify-between">
                  <span class="truncate">{{ subj.name }}</span>
                  <span class="text-xs text-muted-foreground tabular-nums">{{ subj.skill_count }}</span>
                </div>
              </div>
              <p v-if="filteredSubjects.length === 0" class="text-xs text-muted-foreground italic px-2">
                No subjects{{ selectedField ? ' in this field' : '' }}
              </p>
            </div>

            <!-- Bloom level legend -->
            <div class="rounded-xl bg-card shadow-sm p-4">
              <p class="text-[10px] font-semibold text-muted-foreground mb-3 tracking-wider uppercase">Bloom's Levels</p>
              <div class="space-y-1.5">
                <div v-for="level in bloomOrder" :key="level" class="flex items-center gap-2 text-xs">
                  <AppBadge :variant="(bloomColors[level] as any) ?? 'secondary'" class="text-[0.6rem] min-w-[5rem] justify-center">
                    {{ level }}
                  </AppBadge>
                </div>
              </div>
            </div>
          </div>

          <!-- Right: Skill list -->
          <div class="flex-1 min-w-0">
            <div v-if="filteredSkills.length === 0" class="rounded-xl bg-card shadow-sm p-8 text-center">
              <p class="text-sm text-muted-foreground">
                No skills match the current filters.
              </p>
            </div>

            <div v-else class="space-y-2">
              <p class="text-xs text-muted-foreground mb-3">
                {{ filteredSkills.length }} skill{{ filteredSkills.length !== 1 ? 's' : '' }}
              </p>

              <div
                v-for="skill in filteredSkills"
                :key="skill.id"
                class="rounded-xl bg-card shadow-sm p-4 cursor-pointer transition-shadow hover:shadow-md"
                @click="goToSkill(skill.id)"
              >
                <div class="flex items-start justify-between gap-3">
                  <div class="min-w-0 flex-1">
                    <div class="flex items-center gap-2 mb-1">
                      <h3 class="text-sm font-medium text-foreground truncate">
                        {{ skill.name }}
                      </h3>
                      <AppBadge :variant="(bloomColors[skill.bloom_level] as any) ?? 'secondary'" class="text-[0.6rem] flex-shrink-0">
                        {{ skill.bloom_level }}
                      </AppBadge>
                    </div>
                    <p v-if="skill.description" class="text-xs text-muted-foreground line-clamp-2">
                      {{ skill.description }}
                    </p>
                    <div class="flex items-center gap-3 mt-2 text-xs text-muted-foreground">
                      <span v-if="skill.subject_name">{{ skill.subject_field_name }} / {{ skill.subject_name }}</span>
                      <span v-if="skill.prerequisite_count > 0">{{ skill.prerequisite_count }} prerequisite{{ skill.prerequisite_count !== 1 ? 's' : '' }}</span>
                      <span v-if="skill.dependent_count > 0">{{ skill.dependent_count }} dependent{{ skill.dependent_count !== 1 ? 's' : '' }}</span>
                    </div>
                  </div>

                </div>
              </div>
            </div>
          </div>
        </div>
      </div>

      <!-- ============ GRAPH TAB ============ -->
      <div v-if="activeTab === 'graph'">
        <div v-if="forceGraphNodes.length === 0" class="py-16 text-center">
          <div class="mx-auto mb-4 flex h-16 w-16 items-center justify-center rounded-full bg-muted/30">
            <svg class="h-8 w-8 text-muted-foreground/50" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
              <path stroke-linecap="round" stroke-linejoin="round" d="M7.5 21L3 16.5m0 0L7.5 12M3 16.5h13.5m0-13.5L21 7.5m0 0L16.5 12M21 7.5H7.5" />
            </svg>
          </div>
          <h3 class="text-sm font-medium text-foreground">No personal skills to graph yet</h3>
          <p class="mt-1 text-xs text-muted-foreground">
            Complete course assessments to earn proofs; unlocked and prerequisite skills will appear here.
          </p>
        </div>
        <div v-else class="rounded-2xl border border-border overflow-hidden bg-slate-950">
          <div ref="graphContainerRef" class="h-[600px] w-full" />

          <!-- Legend overlay -->
          <div class="flex items-center justify-center gap-6 border-t border-border bg-card px-4 py-3 text-xs">
            <span class="flex items-center gap-1.5">
              <span class="inline-block h-2.5 w-2.5 rounded-full bg-success" />
              <span class="text-muted-foreground">Earned ({{ earnedSkillsCount }})</span>
            </span>
            <span v-if="availableSkillsCount > 0" class="flex items-center gap-1.5">
              <span class="inline-block h-2.5 w-2.5 rounded-full bg-warning" />
              <span class="text-muted-foreground">Available ({{ availableSkillsCount }})</span>
            </span>
            <span class="flex items-center gap-1.5">
              <span class="inline-block h-2 w-2 rounded-full" style="background: rgba(148, 163, 184, 0.4)" />
              <span class="text-muted-foreground">Locked ({{ lockedSkillsCount }})</span>
            </span>
          </div>
        </div>
      </div>

      <!-- ============ CREDENTIALS TAB ============ -->
      <div v-if="activeTab === 'credentials'">
        <div v-if="mySkillCredentials.length === 0" class="py-16 text-center">
          <div class="mx-auto mb-4 flex h-16 w-16 items-center justify-center rounded-full bg-primary/10">
            <svg class="h-8 w-8 text-primary" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
              <path stroke-linecap="round" stroke-linejoin="round" d="M9 12.75L11.25 15 15 9.75M21 12c0 1.268-.63 2.39-1.593 3.068a3.745 3.745 0 01-1.043 3.296 3.745 3.745 0 01-3.296 1.043A3.745 3.745 0 0112 21c-1.268 0-2.39-.63-3.068-1.593a3.746 3.746 0 01-3.296-1.043 3.745 3.745 0 01-1.043-3.296A3.745 3.745 0 013 12c0-1.268.63-2.39 1.593-3.068a3.745 3.745 0 011.043-3.296 3.746 3.746 0 013.296-1.043A3.746 3.746 0 0112 3c1.268 0 2.39.63 3.068 1.593a3.746 3.746 0 013.296 1.043 3.746 3.746 0 011.043 3.296A3.745 3.745 0 0121 12z" />
            </svg>
          </div>
          <h3 class="text-lg font-semibold text-foreground">No credentials yet</h3>
          <p class="mt-1 text-sm text-muted-foreground max-w-md mx-auto">
            Verifiable credentials you earn will appear here. Complete a course whose elements
            can submit a Cardano completion witness to auto-earn your first VC.
          </p>
        </div>

        <div v-else class="grid grid-cols-1 sm:grid-cols-2 gap-4">
          <div
            v-for="vc in mySkillCredentials"
            :key="vc.id"
            class="rounded-xl bg-card shadow-sm p-5 cursor-pointer transition-shadow hover:shadow-md"
            @click="goToSkill((vc.credential_subject.claim as { kind: 'skill'; skill_id: string }).skill_id)"
          >
            <div class="flex items-start justify-between mb-3 gap-3">
              <div class="min-w-0">
                <div class="text-sm font-medium truncate text-foreground">
                  {{ skills.find(s => s.id === (vc.credential_subject.claim as { kind: 'skill'; skill_id: string }).skill_id)?.name ?? (vc.credential_subject.claim as { kind: 'skill'; skill_id: string }).skill_id }}
                </div>
                <AppBadge
                  :variant="(bloomColors[bloomOrder[(vc.credential_subject.claim as { kind: 'skill'; level: number }).level] ?? 'apply'] as any) ?? 'secondary'"
                  class="mt-1.5"
                >
                  {{ bloomOrder[(vc.credential_subject.claim as { kind: 'skill'; level: number }).level] ?? 'apply' }}
                </AppBadge>
              </div>
              <div class="text-right flex-shrink-0">
                <div class="font-mono text-lg font-bold text-primary">
                  {{ (((vc.credential_subject.claim as { kind: 'skill'; score: number }).score) * 100).toFixed(0) }}%
                </div>
                <div class="text-[10px] text-muted-foreground">score</div>
              </div>
            </div>
            <div class="flex flex-wrap gap-1.5 mt-2.5">
              <AppBadge v-if="vc.witness" variant="success" class="text-[0.6rem]">
                on-chain witness
              </AppBadge>
              <AppBadge v-else variant="secondary" class="text-[0.6rem]">
                {{ vc.type[vc.type.length - 1] }}
              </AppBadge>
              <span class="text-[10px] text-muted-foreground font-mono">
                {{ vc.issuance_date.slice(0, 10) }}
              </span>
            </div>
          </div>
        </div>
      </div>
    </template>
  </div>
</template>
