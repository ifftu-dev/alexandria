<script setup lang="ts">
/**
 * The Web skin of one skill: a dot in the field colour (`--field` on an
 * ancestor), filled when earned, a ring on the page background when ready
 * next, a pale tint otherwise. It fills the box `useWebGraph` sizes for the
 * dot's radius and the zoom, less the pointer padding (WEB_HIT_PADDING). It draws no control and no
 * name: the owner wraps it in the skill's button (the Web graph's own, or the
 * explorer's node element in the Web view), which carries the accessible
 * name, and names are drawn in their own layer (`WebGraphNames`), so the skin
 * is hidden from assistive technology.
 */
import type { WebDotState } from './useWebGraph'

defineProps<{ state: WebDotState; selected: boolean; faded: boolean; compact?: boolean }>()
</script>

<template>
  <span class="web-skin" :class="{ 'is-faded': faded, 'is-selected': selected, 'is-compact': compact }" aria-hidden="true">
    <span class="web-dot" :class="`is-${state}`" />
  </span>
</template>

<style scoped>
.web-skin { display: grid; place-items: center; width: calc(100% - 10px); height: calc(100% - 10px); pointer-events: none; transition: opacity var(--transition-base); }
.web-skin.is-faded { opacity: .16; }
.web-dot { box-sizing: border-box; width: 100%; height: 100%; border-radius: 50%; background: color-mix(in srgb, var(--field) 30%, var(--app-background)); }
/* The small graph's pale dots take a stronger tint, so field colour reads at a few pixels. */
.web-skin.is-compact .web-dot { background: color-mix(in srgb, var(--field) 48%, var(--app-background)); }
.web-dot.is-earned, .web-skin.is-compact .web-dot.is-earned { background: var(--field); }
.web-dot.is-ready-next { background: var(--app-background); border: 2px solid var(--field); }
/* The primary-colour selection ring used by the other views. */
.web-skin.is-selected .web-dot { box-shadow: 0 0 0 2px var(--app-background), 0 0 0 4px var(--app-primary); }
</style>
