<script setup lang="ts">
/**
 * The Web graph's lines as an SVG group: thin neutral prerequisite lines
 * with arrowheads at the dependent, and dotted relation lines. Paths are in
 * layout units; `useWebGraph` scales the group by the zoom and the strokes
 * do not scale, so lines keep their width on screen and a zoom rebuilds no
 * path.
 * A group, so it sits in the Web graph's own SVG or in the explorer's canvas
 * alike, at layer point 0, 0. Empty paths are not drawn.
 */
import type { WebLines } from './useWebGraph'

defineProps<{ lines: WebLines; compact?: boolean }>()
const EMPHASES = ['faded', 'plain', 'lit'] as const
</script>

<template>
  <g class="web-lines" :class="{ 'is-compact': compact }" aria-hidden="true">
    <template v-for="emphasis in EMPHASES" :key="emphasis">
      <path v-if="lines.relations[emphasis]" class="web-relation" :class="`is-${emphasis}`" :d="lines.relations[emphasis]" />
      <path v-if="lines.prerequisites[emphasis]" class="web-line" :class="`is-${emphasis}`" :d="lines.prerequisites[emphasis]" />
      <path v-if="lines.heads[emphasis]" class="web-head" :class="`is-${emphasis}`" :d="lines.heads[emphasis]" />
    </template>
  </g>
</template>

<style scoped>
.web-lines { pointer-events: none; transform-box: view-box; transform-origin: 0 0; }
.web-line, .web-relation { vector-effect: non-scaling-stroke; fill: none; stroke: var(--app-muted-foreground); stroke-width: 1; opacity: .45; transition: opacity var(--transition-base); }
.web-relation { stroke-dasharray: 2 3; stroke-linecap: round; }
.web-head { fill: var(--app-muted-foreground); opacity: .45; transition: opacity var(--transition-base); }
.web-line.is-lit, .web-relation.is-lit { stroke: var(--app-foreground); stroke-width: 1.5; opacity: .8; }
.web-head.is-lit { fill: var(--app-foreground); opacity: .8; }
.is-faded { opacity: .1; }
/* The small graph: lighter lines at rest, so the dots and their field colours carry it. */
.web-lines.is-compact .web-line, .web-lines.is-compact .web-relation { stroke-width: .8; opacity: .24; }
.web-lines.is-compact .web-head { opacity: .24; }
.web-lines.is-compact .web-line.is-lit, .web-lines.is-compact .web-relation.is-lit { stroke-width: 1.2; opacity: .8; }
.web-lines.is-compact .web-head.is-lit { opacity: .8; }
.web-lines.is-compact .is-faded { opacity: .08; }
</style>
