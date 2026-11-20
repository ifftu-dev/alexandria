<script setup lang="ts">
/**
 * The Web skin of one skill: a dot in the field colour (`--field` on an
 * ancestor) and, when shown, its name under the dot. Filled when earned, a
 * ring on the page background when ready next, a pale tint otherwise. It
 * draws no control: the owner wraps it in the skill's button (the Web graph's
 * own, or the explorer's node element in the Web view), which carries the
 * accessible name, so the skin is hidden from assistive technology.
 */
import type { WebDotState } from './useWebGraph'
import type { WebNamePlace } from './webGeometry'

withDefaults(defineProps<{ state: WebDotState; size: number; name: string; labelled: boolean; selected: boolean; faded: boolean; compact?: boolean; place?: WebNamePlace }>(), { place: 'below' })
</script>

<template>
  <span class="web-skin" :class="{ 'is-faded': faded, 'is-selected': selected, 'is-compact': compact }" :style="{ '--dot': `${size}px` }" aria-hidden="true">
    <span class="web-dot" :class="`is-${state}`" />
    <span v-if="labelled" class="web-name" :class="`is-${place}`" dir="auto">{{ name }}</span>
  </span>
</template>

<style scoped>
.web-skin { position: relative; display: grid; place-items: center; width: var(--dot); height: var(--dot); pointer-events: none; transition: opacity var(--transition-base); }
.web-skin.is-faded { opacity: .16; }
.web-dot { box-sizing: border-box; width: var(--dot); height: var(--dot); border-radius: 50%; background: color-mix(in srgb, var(--field) 30%, var(--app-background)); }
.web-dot.is-earned { background: var(--field); }
.web-dot.is-ready-next { background: var(--app-background); border: 2px solid var(--field); }
/* The primary-colour selection ring used by the other views. */
.web-skin.is-selected .web-dot { box-shadow: 0 0 0 2px var(--app-background), 0 0 0 4px var(--app-primary); }
/* Constant on-screen size; a background-coloured outline keeps lines from cutting through. */
.web-name { position: absolute; top: calc(100% + 3px); left: 50%; translate: -50% 0; max-width: 160px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 12px; font-weight: 550; line-height: 1.3; color: var(--app-foreground);
  text-shadow: 0 0 2px var(--app-background), 0 0 2px var(--app-background), 0 0 3px var(--app-background), 1px 1px 0 var(--app-background), -1px -1px 0 var(--app-background), 1px -1px 0 var(--app-background), -1px 1px 0 var(--app-background); }
/* The other sides, chosen when the name under the dot would overlap another (gaps match WEB_NAME_GAP and WEB_NAME_SIDE_GAP). */
.web-name.is-above { top: auto; bottom: calc(100% + 3px); }
.web-name.is-right { top: 50%; left: calc(100% + 4px); translate: 0 -50%; }
.web-name.is-left { top: 50%; left: auto; right: calc(100% + 4px); translate: 0 -50%; }
.web-skin.is-compact .web-name { font-size: 10px; max-width: 110px; }
.web-skin.is-selected .web-name { color: var(--app-primary); font-weight: 650; }
</style>
