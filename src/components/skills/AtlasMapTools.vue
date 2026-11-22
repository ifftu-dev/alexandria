<script setup lang="ts">
/**
 * The map's zoom and pan buttons, shared by the Detailed map (a toolbar row
 * above the map) and the Web graph (a small group over the canvas corner,
 * with Fit). Each press is an event; the owner moves its own camera.
 */
import { useI18n } from 'vue-i18n'

withDefaults(defineProps<{ percent: number; canZoomIn: boolean; canZoomOut: boolean; fit?: boolean; overlay?: boolean }>(), { fit: false, overlay: false })
const emit = defineEmits<{ zoom: [direction: 1 | -1]; fit: []; pan: [x: -1 | 0 | 1, y: -1 | 0 | 1] }>()
const { t } = useI18n()
const PANS = [
  { label: 'skills.atlas.panLeft', symbol: '←', x: -1, y: 0 },
  { label: 'skills.atlas.panUp', symbol: '↑', x: 0, y: -1 },
  { label: 'skills.atlas.panDown', symbol: '↓', x: 0, y: 1 },
  { label: 'skills.atlas.panRight', symbol: '→', x: 1, y: 0 },
] as const
</script>

<template>
  <div class="map-tools" :class="{ 'is-overlay': overlay }" :aria-label="t('skills.atlas.map')">
    <button type="button" :aria-label="t('skills.atlas.zoomIn')" :disabled="!canZoomIn" @click="emit('zoom', 1)">+</button>
    <button type="button" :aria-label="t('skills.atlas.zoomOut')" :disabled="!canZoomOut" @click="emit('zoom', -1)">−</button>
    <span>{{ percent }}%</span>
    <button v-if="fit" type="button" @click="emit('fit')">{{ t('skills.atlas.webFit') }}</button>
    <button v-for="pan in PANS" :key="pan.label" type="button" :aria-label="t(pan.label)" @click="emit('pan', pan.x, pan.y)">{{ pan.symbol }}</button>
  </div>
</template>

<style scoped>
.map-tools { display: flex; flex-wrap: wrap; align-items: center; gap: 4px; padding: 8px; border-block-end: 1px solid var(--app-border); }
.map-tools span { font-size: 12px; margin-inline: 6px; }
.map-tools button { min-width: 44px; min-height: 44px; padding: 8px 12px; border: 1px solid var(--app-border); border-radius: 9px; background: var(--app-card); color: inherit; font: inherit; text-align: center; cursor: pointer; }
.map-tools button:hover { background: var(--app-muted); }
.map-tools button:disabled { opacity: .45; cursor: default; }
.map-tools button:focus-visible { outline: 3px solid var(--app-ring); outline-offset: 2px; }
/* Over the canvas corner: a small card of zoom, percentage and Fit above the four pan arrows. */
.map-tools.is-overlay { position: absolute; inset-block-end: 10px; inset-inline-end: 10px; z-index: 1; display: grid; grid-template-columns: repeat(4, auto); gap: 3px; padding: 4px; border: 1px solid var(--app-border); border-radius: 10px; background: color-mix(in srgb, var(--app-card) 92%, transparent); box-shadow: 0 2px 10px color-mix(in srgb, var(--app-foreground) 8%, transparent); }
.map-tools.is-overlay span { margin: 0; min-width: 0; font-size: 11px; text-align: center; color: var(--app-muted-foreground); }
.map-tools.is-overlay button { min-width: 32px; min-height: 32px; padding: 2px 6px; border-radius: 7px; font-size: 13px; line-height: 1; }
@media (pointer: coarse) {
  .map-tools, .map-tools.is-overlay { display: none; }
}
</style>
