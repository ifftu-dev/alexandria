<script setup lang="ts">
/**
 * SidebarSkillGraph — compact force-directed skill graph in the sidebar.
 *
 * Compact force-directed skill graph using `force-graph` library,
 * node coloring (earned=green, available=yellow, locked=gray),
 * glow effects, and legend.
 *
 * Data fetched once via Tauri IPC and cached in useSkillGraphState().
 */
import { ref, computed, onMounted, onBeforeUnmount, watch, nextTick } from 'vue'
import { useRouter } from 'vue-router'
import { useLocalApi } from '@/composables/useLocalApi'
import { useSkillGraphState, type SkillStatus } from '@/composables/useSkillGraphState'
import type { SkillInfo, SkillGraphEdge, SkillProof } from '@/types'
import SkillGraphModal from '@/components/layout/SkillGraphModal.vue'

const router = useRouter()
const { invoke } = useLocalApi()

const containerRef = ref<HTMLElement | null>(null)
// eslint-disable-next-line @typescript-eslint/no-explicit-any
const graphInstance = ref<any>(null)

const {
  skills,
  edges,
  proofs,
  earnedSkillIds,
  earnedCount,
  availableCount,
  lockedCount,
  totalCount,
  loaded,
  isModalOpen,
} = useSkillGraphState()

const prereqMap = computed(() => {
  const map = new Map<string, string[]>()
  for (const e of edges.value) {
    if (!map.has(e.skill_id)) map.set(e.skill_id, [])
    map.get(e.skill_id)!.push(e.prerequisite_id)
  }
  return map
})

const miniGraphNodes = computed(() => {
  const earned = earnedSkillIds.value
  return skills.value.map((skill) => {
    const prereqs = prereqMap.value.get(skill.id) ?? []
    const status: SkillStatus = earned.has(skill.id)
      ? 'earned'
      : (prereqs.length === 0 || prereqs.every(p => earned.has(p)))
          ? 'available'
          : 'locked'
    return {
      id: skill.id,
      name: skill.name,
      routeId: skill.id,
      status,
      prerequisites: prereqs,
    }
  })
})

async function loadData() {
  if (loaded.value) return

  try {
    await invoke<number>('bootstrap_public_taxonomy').catch(() => 0)

    const [sk, edgeList, proofList] = await Promise.all([
      invoke<SkillInfo[]>('list_skills', {}),
      invoke<SkillGraphEdge[]>('list_skill_graph_edges', {}),
      invoke<SkillProof[]>('list_skill_proofs', {}),
    ])

    skills.value = sk
    edges.value = edgeList
    proofs.value = proofList
    totalCount.value = sk.length

    // Build earned set from proofs
    const earnedIds = new Set(proofList.map(p => p.skill_id))
    earnedSkillIds.value = earnedIds
    earnedCount.value = earnedIds.size

    // Build prerequisite map to determine available vs locked
    const prereqMap = new Map<string, string[]>()
    for (const e of edgeList) {
      if (!prereqMap.has(e.skill_id)) prereqMap.set(e.skill_id, [])
      prereqMap.get(e.skill_id)!.push(e.prerequisite_id)
    }

    let avail = 0
    let lock = 0
    for (const skill of sk) {
      if (earnedIds.has(skill.id)) continue
      const prereqs = prereqMap.get(skill.id) ?? []
      const allMet = prereqs.length === 0 || prereqs.every(p => earnedIds.has(p))
      if (allMet) {
        avail++
      } else {
        lock++
      }
    }
    availableCount.value = avail
    lockedCount.value = lock
  } catch (e) {
    console.error('SidebarSkillGraph: failed to load data:', e)
  }

  loaded.value = true
  await nextTick()
  initGraph()
}

onMounted(() => {
  loadData()
})

// If data was already loaded (e.g. navigated away and back), re-init the canvas
watch(loaded, (val) => {
  if (val && containerRef.value && !graphInstance.value) {
    nextTick(() => initGraph())
  }
})

async function initGraph() {
  if (!containerRef.value || !skills.value.length) return

  // Clean up existing instance
  if (graphInstance.value) {
    graphInstance.value._destructor?.()
    graphInstance.value = null
  }

  // force-graph exports a class but is callable as a factory at runtime
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const ForceGraph = (await import('force-graph')).default as any

  const nodes = miniGraphNodes.value

  const links: Array<{ source: string; target: string }> = []
  const nodeIds = new Set(skills.value.map(s => s.id))
  for (const edge of edges.value) {
    if (nodeIds.has(edge.skill_id) && nodeIds.has(edge.prerequisite_id)) {
      links.push({ source: edge.prerequisite_id, target: edge.skill_id })
    }
  }

  const width = containerRef.value.clientWidth
  const height = containerRef.value.clientHeight

  const graph = ForceGraph()(containerRef.value)
    .width(width)
    .height(height)
    .graphData({ nodes, links })
    .nodeCanvasObject((node: Record<string, unknown>, ctx: CanvasRenderingContext2D) => {
      const status = (node as { status?: string }).status ?? 'locked'
      const isEarned = status === 'earned'
      const isAvailable = status === 'available'
      const r = isEarned ? 3 : isAvailable ? 2.5 : 2
      const x = node.x as number
      const y = node.y as number

      // Glow for earned nodes
      if (isEarned) {
        ctx.beginPath()
        ctx.arc(x, y, r + 2, 0, 2 * Math.PI)
        ctx.fillStyle = 'rgba(34, 197, 94, 0.2)'
        ctx.fill()
      }

      // Glow for available nodes
      if (isAvailable) {
        ctx.beginPath()
        ctx.arc(x, y, r + 1.5, 0, 2 * Math.PI)
        ctx.fillStyle = 'rgba(234, 179, 8, 0.15)'
        ctx.fill()
      }

      // Node circle: earned=green, available=yellow, locked=gray
      ctx.beginPath()
      ctx.arc(x, y, r, 0, 2 * Math.PI)
      ctx.fillStyle = isEarned ? '#22c55e' : isAvailable ? '#eab308' : 'rgba(100, 116, 139, 0.4)'
      ctx.fill()
    })
    .linkColor(() => 'rgba(148, 163, 184, 0.1)')
    .linkWidth(0.5)
    .backgroundColor('transparent')
    .enableZoomInteraction(false)
    .enablePanInteraction(false)
    .enableNodeDrag(false)
    .cooldownTicks(60)
    .onEngineStop(() => {
      graph.zoomToFit(0, 8)
    })
    .onNodeClick((node: Record<string, unknown>) => {
      const routeId = String(node.routeId ?? node.id ?? '')
      if (routeId) router.push(`/skills/${routeId}`)
    })

  // Tighter forces for compact sidebar layout
  graph.d3Force('charge')?.strength(-12)
  graph.d3Force('link')?.distance(15)

  graphInstance.value = graph
}

onBeforeUnmount(() => {
  if (graphInstance.value) {
    graphInstance.value._destructor?.()
    graphInstance.value = null
  }
})
</script>

<template>
  <div v-if="loaded && totalCount > 0">
    <div class="relative">
      <div
        ref="containerRef"
        class="h-[180px] w-full cursor-pointer overflow-hidden rounded-lg"
        style="background: color-mix(in srgb, var(--app-foreground) 3%, transparent)"
        title="Double-click to expand"
        @dblclick.stop="isModalOpen = true"
      />

      <button
        title="Expand skill graph"
        class="absolute right-1.5 top-1.5 z-10 rounded-md p-1 text-muted-foreground backdrop-blur-sm transition-colors"
        style="background: color-mix(in srgb, var(--app-muted) 88%, transparent)"
        @click.stop="isModalOpen = true"
      >
        <svg class="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
          <path stroke-linecap="round" stroke-linejoin="round" d="M3.75 3.75v4.5m0-4.5h4.5m-4.5 0L9 9M3.75 20.25v-4.5m0 4.5h4.5m-4.5 0L9 15M20.25 3.75h-4.5m4.5 0v4.5m0-4.5L15 9m5.25 11.25h-4.5m4.5 0v-4.5m0 4.5L15 15" />
        </svg>
      </button>
    </div>

    <!-- Legend -->
    <div class="mt-2 flex items-center justify-center gap-3 text-[10px]">
      <span class="flex items-center gap-1" title="You have a skill proof for these skills">
        <span class="inline-block h-2 w-2 rounded-full bg-success" />
        <span class="text-muted-foreground">Earned ({{ earnedCount }})</span>
      </span>
      <span v-if="availableCount > 0" class="flex items-center gap-1" title="All prerequisites met">
        <span class="inline-block h-2 w-2 rounded-full bg-warning" />
        <span class="text-muted-foreground">Available ({{ availableCount }})</span>
      </span>
      <span class="flex items-center gap-1" title="Prerequisites still unearned">
        <span class="inline-block h-1.5 w-1.5 rounded-full" style="background: rgba(100, 116, 139, 0.4)" />
        <span class="text-muted-foreground">Locked ({{ lockedCount }})</span>
      </span>
    </div>

    <!-- Summary link -->
    <div class="mt-1 text-center">
      <button class="text-[10px] text-muted-foreground transition-colors hover:text-primary" @click="router.push('/skills')">
        {{ earnedCount }} / {{ totalCount }} skills
      </button>
    </div>

    <SkillGraphModal
      :visible="isModalOpen"
      :nodes="miniGraphNodes"
      :earned-count="earnedCount"
      :available-count="availableCount"
      :locked-count="lockedCount"
      :total-count="totalCount"
      @close="isModalOpen = false"
    />
  </div>

  <!-- Loading state -->
  <div v-else-if="!loaded" class="flex items-center justify-center py-6">
    <div class="spinner" />
  </div>
</template>
