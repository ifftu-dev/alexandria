<script setup lang="ts">
interface Tab {
  key: string
  label: string
  count?: number
}

interface Props {
  tabs: Tab[]
  modelValue: string
}

defineProps<Props>()
defineEmits<{ 'update:modelValue': [key: string] }>()
</script>

<template>
  <div class="tabs">
    <button
      v-for="tab in tabs"
      :key="tab.key"
      class="tab"
      :class="{ 'tab-active': modelValue === tab.key }"
      @click="$emit('update:modelValue', tab.key)"
    >
      {{ tab.label }}
      <span
        v-if="tab.count !== undefined"
        class="ml-1 text-xs opacity-70"
      >{{ tab.count }}</span>
    </button>
  </div>
</template>
