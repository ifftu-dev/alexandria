<script setup lang="ts">
interface Props {
  variant?: 'primary' | 'secondary' | 'success' | 'warning' | 'error' | 'accent' | 'governance'
}

withDefaults(defineProps<Props>(), {
  variant: 'primary',
})
</script>

<template>
  <span class="badge" :class="`badge-${variant}`">
    <slot />
  </span>
</template>
