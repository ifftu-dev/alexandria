<script setup lang="ts">
interface Props {
  variant?: 'error' | 'success' | 'warning' | 'info'
}

withDefaults(defineProps<Props>(), {
  variant: 'info',
})
</script>

<template>
  <div class="alert" :class="`alert-${variant}`">
    <slot />
  </div>
</template>
