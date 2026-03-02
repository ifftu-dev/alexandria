<script setup lang="ts">
interface Props {
  label: string
  mono?: boolean
}

withDefaults(defineProps<Props>(), {
  mono: false,
})
</script>

<template>
  <div class="flex items-start gap-2 text-sm">
    <span class="text-muted-foreground w-28 shrink-0">{{ label }}</span>
    <span :class="mono ? 'font-mono text-xs break-all' : ''">
      <slot />
    </span>
  </div>
</template>
