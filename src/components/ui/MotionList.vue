<script setup lang="ts">
import { holdLeavingPlace } from '@/composables/useMotion'

/**
 * A list whose items enter, leave and move as `motion-list` (main.css). The
 * container is positioned so a leaving item is held where it stood while the
 * rest move into place. Reduced and Off show and remove items at once.
 * Classes and attributes land on the rendered container.
 */
withDefaults(defineProps<{
  /** The container element. */
  tag?: string
  /** Items present on mount also enter. */
  appear?: boolean
}>(), { tag: 'div', appear: false })
</script>

<template>
  <TransitionGroup name="motion-list" :tag="tag" :appear="appear" class="relative" @before-leave="holdLeavingPlace">
    <slot />
  </TransitionGroup>
</template>
