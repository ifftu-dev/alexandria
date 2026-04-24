<script setup lang="ts">
/**
 * SkillGraph — SVG-based DAG visualization for skill prerequisites.
 *
 * Uses a simple layered layout algorithm:
 * 1. Topological sort to determine depth (layer) of each node
 * 2. Within each layer, nodes are spaced evenly
 * 3. Edges are drawn as curved paths from prerequisite to dependent
 *
 * Nodes are colored by Bloom's taxonomy level. Proven skills get a
 * highlighted ring. Click a node to navigate to its detail page.
 */
import { ref, computed } from 'vue'
import type { SkillInfo, SkillGraphEdge } from '@/types'

const props = defineProps<{
  skills: SkillInfo[]
  edges: SkillGraphEdge[]
}>()

const emit = defineEmits<{
  select: [skillId: string]
}>()

const hoveredNode = ref<string | null>(null)

// Bloom level colors (RGB values matching design tokens)
const bloomFills: Record<string, string> = {
  remember: '#94a3b8',
  understand: '#6366f1',
  apply: '#a855f7',
  analyze: '#f59e0b',
  evaluate: '#10b981',
  create: '#e11d48',
}

interface LayoutNode {
  id: string
  name: string
  bloom: string
  layer: number
  x: number
  y: number
}

interface LayoutEdge {
  from: string
  to: string
  x1: number
  y1: number
  x2: number
  y2: number
}

const NODE_W = 140
const NODE_H = 36
const LAYER_GAP = 100
const NODE_GAP = 24
const PADDING = 40

const layout = computed(() => {
  const skillMap = new Map(props.skills.map(s => [s.id, s]))

  // Build adjacency: prerequisite -> [dependents]
  const dependents = new Map<string, string[]>()
  const prerequisites = new Map<string, string[]>()
  const nodeIds = new Set(props.skills.map(s => s.id))

  for (const e of props.edges) {
    if (!nodeIds.has(e.skill_id) || !nodeIds.has(e.prerequisite_id)) continue
    if (!dependents.has(e.prerequisite_id)) dependents.set(e.prerequisite_id, [])
    dependents.get(e.prerequisite_id)!.push(e.skill_id)
    if (!prerequisites.has(e.skill_id)) prerequisites.set(e.skill_id, [])
    prerequisites.get(e.skill_id)!.push(e.prerequisite_id)
  }

  // Compute layers via longest path from roots
  const layers = new Map<string, number>()

  function computeLayer(id: string, visited: Set<string>): number {
    if (layers.has(id)) return layers.get(id)!
    if (visited.has(id)) return 0 // cycle protection
    visited.add(id)

    const prereqs = prerequisites.get(id) ?? []
    const maxPrereqLayer = prereqs.length > 0
      ? Math.max(...prereqs.map(p => computeLayer(p, visited)))
      : -1

    const layer = maxPrereqLayer + 1
    layers.set(id, layer)
    return layer
  }

  for (const id of nodeIds) {
    computeLayer(id, new Set())
  }

  // Group by layer
  const layerGroups = new Map<number, string[]>()
  for (const [id, layer] of layers) {
    if (!layerGroups.has(layer)) layerGroups.set(layer, [])
    layerGroups.get(layer)!.push(id)
  }

  // Sort layers
  const sortedLayers = [...layerGroups.keys()].sort((a, b) => a - b)

  // Position nodes
  const nodes: LayoutNode[] = []
  const positions = new Map<string, { x: number; y: number }>()

  const maxNodesInLayer = Math.max(1, ...sortedLayers.map(l => (layerGroups.get(l) ?? []).length))

  for (const layer of sortedLayers) {
    const ids = layerGroups.get(layer) ?? []
    // Sort alphabetically within layer for stable layout
    ids.sort((a, b) => {
      const sa = skillMap.get(a)?.name ?? a
      const sb = skillMap.get(b)?.name ?? b
      return sa.localeCompare(sb)
    })

    const totalWidth = ids.length * NODE_W + (ids.length - 1) * NODE_GAP
    const startX = PADDING + (maxNodesInLayer * NODE_W + (maxNodesInLayer - 1) * NODE_GAP - totalWidth) / 2

    for (let i = 0; i < ids.length; i++) {
      const id = ids[i]!
      const skill = skillMap.get(id)
      const x = startX + i * (NODE_W + NODE_GAP)
      const y = PADDING + layer * (NODE_H + LAYER_GAP)

      positions.set(id, { x: x + NODE_W / 2, y: y + NODE_H / 2 })

      nodes.push({
        id,
        name: skill?.name ?? id,
        bloom: skill?.bloom_level ?? 'apply',
        layer,
        x,
        y,
      })
    }
  }

  // Position edges
  const edgeLayouts: LayoutEdge[] = []
  for (const e of props.edges) {
    const from = positions.get(e.prerequisite_id)
    const to = positions.get(e.skill_id)
    if (!from || !to) continue

    edgeLayouts.push({
      from: e.prerequisite_id,
      to: e.skill_id,
      x1: from.x,
      y1: from.y + NODE_H / 2,
      x2: to.x,
      y2: to.y - NODE_H / 2,
    })
  }

  // Compute SVG dimensions
  const width = Math.max(400, maxNodesInLayer * NODE_W + (maxNodesInLayer - 1) * NODE_GAP + PADDING * 2)
  const height = Math.max(200, sortedLayers.length * (NODE_H + LAYER_GAP) + PADDING * 2 - LAYER_GAP + NODE_H)

  return { nodes, edges: edgeLayouts, width, height }
})

function edgePath(e: LayoutEdge): string {
  const dy = e.y2 - e.y1
  const cp = dy * 0.4
  return `M ${e.x1} ${e.y1} C ${e.x1} ${e.y1 + cp}, ${e.x2} ${e.y2 - cp}, ${e.x2} ${e.y2}`
}

function isEdgeHighlighted(e: LayoutEdge): boolean {
  return hoveredNode.value === e.from || hoveredNode.value === e.to
}
</script>

<template>
  <div class="card overflow-hidden">
    <div class="p-3 border-b border-border flex items-center justify-between">
      <div class="text-xs text-muted-foreground">
        {{ layout.nodes.length }} skills, {{ layout.edges.length }} prerequisite edges
      </div>
      <div class="flex items-center gap-3">
        <div v-for="(color, level) in bloomFills" :key="level" class="flex items-center gap-1">
          <span class="w-2.5 h-2.5 rounded-full" :style="{ backgroundColor: color }" />
          <span class="text-[0.6rem] text-muted-foreground">{{ level }}</span>
        </div>
      </div>
    </div>

    <div class="overflow-auto bg-muted/15" style="max-height: 600px">
      <svg
        :width="layout.width"
        :height="layout.height"
        class="select-none"
      >
        <!-- Grid pattern -->
        <defs>
          <pattern id="grid" width="20" height="20" patternUnits="userSpaceOnUse">
            <circle cx="10" cy="10" r="0.5" fill="var(--app-border)" opacity="0.3" />
          </pattern>
          <!-- Arrow marker -->
          <marker id="arrow" viewBox="0 0 10 10" refX="10" refY="5"
            markerWidth="6" markerHeight="6" orient="auto-start-reverse">
            <path d="M 0 0 L 10 5 L 0 10 z" fill="var(--app-muted-foreground)" opacity="0.5" />
          </marker>
          <marker id="arrow-hl" viewBox="0 0 10 10" refX="10" refY="5"
            markerWidth="6" markerHeight="6" orient="auto-start-reverse">
            <path d="M 0 0 L 10 5 L 0 10 z" fill="var(--app-primary)" opacity="0.8" />
          </marker>
        </defs>

        <rect width="100%" height="100%" fill="url(#grid)" />

        <!-- Edges -->
        <g>
          <path
            v-for="(e, i) in layout.edges"
            :key="`edge-${i}`"
            :d="edgePath(e)"
            fill="none"
            :stroke="isEdgeHighlighted(e) ? 'var(--app-primary)' : 'var(--app-muted-foreground)'"
            :stroke-width="isEdgeHighlighted(e) ? 2 : 1"
            :opacity="isEdgeHighlighted(e) ? 0.8 : 0.25"
            :marker-end="isEdgeHighlighted(e) ? 'url(#arrow-hl)' : 'url(#arrow)'"
            class="transition-all duration-200"
          />
        </g>

        <!-- Nodes -->
        <g
          v-for="node in layout.nodes"
          :key="node.id"
          class="cursor-pointer"
          @click="emit('select', node.id)"
          @mouseenter="hoveredNode = node.id"
          @mouseleave="hoveredNode = null"
        >
          <!-- Node background -->
          <rect
            :x="node.x"
            :y="node.y"
            :width="NODE_W"
            :height="NODE_H"
            rx="8"
            :fill="bloomFills[node.bloom] ?? '#6366f1'"
            :opacity="hoveredNode === node.id ? 1 : 0.85"
            class="transition-opacity duration-150"
          />

          <!-- Node label -->
          <text
            :x="node.x + NODE_W / 2"
            :y="node.y + NODE_H / 2"
            text-anchor="middle"
            dominant-baseline="central"
            fill="white"
            font-size="11"
            font-weight="500"
            class="pointer-events-none"
          >
            <tspan>{{ node.name.length > 18 ? node.name.slice(0, 16) + '...' : node.name }}</tspan>
          </text>

        </g>
      </svg>
    </div>
  </div>
</template>
