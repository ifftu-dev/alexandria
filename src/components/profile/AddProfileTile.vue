<script setup lang="ts">
defineEmits<{
  click: []
}>()
</script>

<template>
  <button
    type="button"
    class="group flex flex-col items-center gap-3 p-4 rounded-2xl transition-all hover:bg-surface/40 hover:scale-105 active:scale-100 focus:outline-none focus-visible:ring-2 focus-visible:ring-primary focus-visible:ring-offset-2 focus-visible:ring-offset-background"
    @click="$emit('click')"
  >
    <div
      class="w-28 h-28 rounded-full border-2 border-dashed border-border flex items-center justify-center text-3xl text-muted-foreground group-hover:border-primary group-hover:text-primary transition-colors"
    >
      +
    </div>
    <div class="text-center">
      <div class="font-medium text-foreground">Add user</div>
      <div class="text-xs text-muted-foreground mt-0.5">Create a new profile</div>
    </div>
  </button>
</template>
