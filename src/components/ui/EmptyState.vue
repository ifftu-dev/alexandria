<script setup lang="ts">
interface Props {
  title?: string
  description?: string
  icon?: string
}

withDefaults(defineProps<Props>(), {
  title: 'Nothing here yet',
  description: '',
  icon: '',
})
</script>

<template>
  <div class="card p-8 text-center">
    <div v-if="icon" class="text-2xl mb-3 opacity-50">{{ icon }}</div>
    <p class="text-sm font-medium text-foreground mb-1">{{ title }}</p>
    <p v-if="description" class="text-xs text-muted-foreground">{{ description }}</p>
    <div v-if="$slots.action" class="mt-4">
      <slot name="action" />
    </div>
  </div>
</template>
