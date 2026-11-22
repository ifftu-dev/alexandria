<script setup lang="ts">
/**
 * The Web graph's names, in their own layer above the dots: each one in a
 * box the size of its dot, at the dot's centre in layer pixels, with the name
 * on the side `useWebGraph` chose. The layer moves with the pan like the
 * dots; the names re-render when the camera changes, the dots do not.
 */
import type { WebName } from './useWebGraph'

defineProps<{ names: readonly WebName[]; compact?: boolean }>()
</script>

<template>
  <div class="web-names" :class="{ 'is-compact': compact }" aria-hidden="true">
    <span v-for="name in names" :key="name.id" class="web-name-box" :class="{ 'is-faded': name.faded, 'is-selected': name.selected }" :data-name-for="name.id" :style="{ transform: `translate(${name.left - name.dot / 2}px, ${name.top - name.dot / 2}px)`, '--dot': `${name.dot}px`, '--name-offset': `${name.offset}px` }">
      <span class="web-name" :class="`is-${name.place}`" dir="auto">{{ name.name }}</span>
    </span>
  </div>
</template>

<style scoped>
.web-names { position: absolute; top: 0; left: 0; width: 0; height: 0; pointer-events: none; }
.web-name-box { position: absolute; top: 0; left: 0; width: var(--dot); height: var(--dot); transition: opacity var(--transition-base); }
.web-name-box.is-faded { opacity: .16; }
/* Constant on-screen size; a background-coloured outline keeps lines from cutting through. */
.web-name { position: absolute; top: calc(100% + 3px + var(--name-offset, 0px)); left: 50%; translate: -50% 0; max-width: 160px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 12px; font-weight: 550; line-height: 1.3; color: var(--app-foreground);
  text-shadow: 0 0 2px var(--app-background), 0 0 2px var(--app-background), 0 0 3px var(--app-background), 1px 1px 0 var(--app-background), -1px -1px 0 var(--app-background), 1px -1px 0 var(--app-background), -1px 1px 0 var(--app-background); }
/* The other sides, chosen when the name under the dot would overlap another, and the offset further out when no side is free (gaps match WEB_NAME_GAP and WEB_NAME_SIDE_GAP). */
.web-name.is-above { top: auto; bottom: calc(100% + 3px + var(--name-offset, 0px)); }
.web-name.is-right { top: 50%; left: calc(100% + 4px + var(--name-offset, 0px)); translate: 0 -50%; }
.web-name.is-left { top: 50%; left: auto; right: calc(100% + 4px + var(--name-offset, 0px)); translate: 0 -50%; }
.web-names.is-compact .web-name { font-size: 10px; max-width: 110px; }
.web-name-box.is-selected .web-name { color: var(--app-primary); font-weight: 650; }
</style>
