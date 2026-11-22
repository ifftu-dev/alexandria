<script setup lang="ts">
/**
 * The Web graph's dots as an SVG group, in layout units, which `useWebGraph`
 * scales by the zoom: a pan or zoom redraws them without a render or a
 * restyle. Each layer holds the dots of one field colour, state and emphasis
 * as two paths:
 * - discs of the dot's radius, which scale with the zoom (the dot's size is
 *   the Bloom level earned);
 * - zero-length round-capped strokes at the dots' centres, under the discs,
 *   which do not scale, so a dot is never drawn smaller than the smallest
 *   size (a ready-next ring larger than that hides its cap).
 * Earned dots are filled in the field colour, ready-next dots are a ring on
 * the page background, and the rest a pale tint. The selected dot is drawn
 * again on top inside the primary-colour selection ring: a ring and a gap
 * that each layer draws as a non-scaling stroke around the disc and a wider
 * cap, so the ring follows whichever of the two is larger.
 */
import type { WebDot, WebDotLayer } from './useWebGraph'

defineProps<{ layers: readonly WebDotLayer[]; selected: { dot: WebDot; disc: string; cap: string } | null; compact?: boolean }>()
</script>

<template>
  <g class="web-dots web-scaled" :class="{ 'is-compact': compact }" aria-hidden="true">
    <g v-for="layer in layers" :key="layer.key" class="web-dot-layer" :class="[`is-${layer.state}`, { 'is-faded': layer.faded }]" :style="{ '--field': layer.color }">
      <path class="web-cap" :d="layer.caps" />
      <path class="web-disc" :d="layer.discs" />
    </g>
    <g v-if="selected" class="web-dot-layer is-selected" :class="`is-${selected.dot.state}`" :style="{ '--field': selected.dot.color }">
      <path class="web-ring" :d="selected.disc" /><path class="web-ring web-cap-ring" :d="selected.cap" />
      <path class="web-gap" :d="selected.disc" /><path class="web-gap web-cap-gap" :d="selected.cap" />
      <path class="web-cap" :d="selected.cap" /><path class="web-disc" :d="selected.disc" />
    </g>
  </g>
</template>

<style scoped>
.web-dots { pointer-events: none; transform-box: view-box; transform-origin: 0 0; --web-dot-min: 4px; }
.web-dots.is-compact { --web-dot-min: 6px; }
.web-dot-layer { transition: opacity var(--transition-base); }
.web-dot-layer.is-faded { opacity: .16; }
/* Pale: a tint of the field colour; the small graph takes a stronger one, so field colour reads at a few pixels. */
.web-disc { fill: color-mix(in srgb, var(--field) 30%, var(--app-background)); stroke: none; }
.web-cap { fill: none; stroke: color-mix(in srgb, var(--field) 30%, var(--app-background)); stroke-width: var(--web-dot-min); stroke-linecap: round; vector-effect: non-scaling-stroke; }
.web-dots.is-compact .web-disc { fill: color-mix(in srgb, var(--field) 48%, var(--app-background)); }
.web-dots.is-compact .web-cap { stroke: color-mix(in srgb, var(--field) 48%, var(--app-background)); }
.is-earned .web-disc { fill: var(--field); }
.is-earned .web-cap { stroke: var(--field); }
/* Ready next: a ring on the page background (match WebGraphDot's 2-pixel border). */
.is-ready-next .web-disc { fill: var(--app-background); stroke: var(--field); stroke-width: 2px; vector-effect: non-scaling-stroke; }
.is-ready-next .web-cap { stroke: var(--field); }
/* The selection ring: 2 pixels of page background, then 2 of the primary colour, past the dot's edge. */
.web-ring, .web-gap { fill: none; stroke-linecap: round; vector-effect: non-scaling-stroke; }
.web-ring { stroke: var(--app-primary); stroke-width: 8px; }
.web-gap { stroke: var(--app-background); stroke-width: 4px; }
.web-cap-ring { stroke-width: calc(var(--web-dot-min) + 8px); }
.web-cap-gap { stroke-width: calc(var(--web-dot-min) + 4px); }
</style>
