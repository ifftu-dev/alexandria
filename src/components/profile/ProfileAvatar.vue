<script setup lang="ts">
import { computed } from 'vue'

import type { Avatar } from '@/types'

interface Props {
  avatar: Avatar
  /** Display name fallback for identicon initials. */
  fallbackName: string
  /** Background color (CSS hex). */
  color: string
  /** Tile size in pixels. */
  size?: number
}

const props = withDefaults(defineProps<Props>(), {
  size: 96,
})

const initials = computed(() => {
  const parts = props.fallbackName
    .trim()
    .split(/\s+/)
    .filter((p) => p.length > 0)
    .map((p) => p.charAt(0).toUpperCase())
  if (parts.length === 0) return '?'
  if (parts.length === 1) return parts[0] ?? '?'
  const first = parts[0] ?? ''
  const last = parts[parts.length - 1] ?? ''
  return first + last
})

const fontSize = computed(() => `${Math.round(props.size * 0.42)}px`)
</script>

<template>
  <div
    class="rounded-full flex items-center justify-center select-none overflow-hidden border border-border/40 shadow-sm"
    :style="{
      width: `${size}px`,
      height: `${size}px`,
      backgroundColor: color,
      fontSize,
    }"
  >
    <template v-if="avatar.kind === 'emoji'">
      <span class="leading-none">{{ avatar.value }}</span>
    </template>
    <template v-else-if="avatar.kind === 'identicon'">
      <span class="text-white font-semibold leading-none">{{ initials }}</span>
    </template>
    <template v-else>
      <!-- Image avatars resolve through the asset protocol once profile is unlocked. -->
      <span class="text-white font-semibold leading-none">{{ initials }}</span>
    </template>
  </div>
</template>
