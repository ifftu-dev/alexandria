<script setup lang="ts">
interface Props {
  size?: 'sm' | 'md' | 'lg'
  label?: string
}

withDefaults(defineProps<Props>(), {
  size: 'md',
  label: '',
})

const sizeMap = {
  sm: 'w-4 h-4 border-[1.5px]',
  md: 'w-6 h-6 border-2',
  lg: 'w-8 h-8 border-2',
}
</script>

<template>
  <div class="flex items-center gap-2">
    <div
      class="rounded-full animate-spin border-[rgb(var(--color-primary))] border-t-transparent"
      :class="sizeMap[size]"
    />
    <span v-if="label" class="text-sm text-[rgb(var(--color-muted-foreground))]">{{ label }}</span>
  </div>
</template>
