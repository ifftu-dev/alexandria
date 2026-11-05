<script setup lang="ts" generic="T">
/**
 * Renders its owner's slot from `source.items`, read here rather than in the
 * owner. A change to the items re-renders this layer only, not the component
 * that owns it; the slot keeps the owner's scoped styles.
 */
defineProps<{ source: { readonly items: readonly T[] } }>()
defineSlots<{ default(props: { items: readonly T[] }): unknown }>()
</script>

<template>
  <slot :items="source.items" />
</template>
