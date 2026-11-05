<script setup lang="ts">
import { computed, nextTick, ref } from 'vue'

interface Tab {
  key: string
  label: string
  count?: number
}

interface Props {
  tabs: Tab[]
  modelValue: string
}

const props = defineProps<Props>()
const emit = defineEmits<{ 'update:modelValue': [key: string] }>()

const list = ref<HTMLElement | null>(null)
/** The tab in the tab order: the selected one, or the first if none is. */
const focusable = computed(() => props.tabs.some(tab => tab.key === props.modelValue) ? props.modelValue : props.tabs[0]?.key)

/**
 * WAI-ARIA tabs with automatic activation: the selected tab is the only one in
 * the tab order, and the arrow keys (mirrored right-to-left), Home and End
 * move focus and selection together, wrapping at the ends.
 */
function onKeydown(event: KeyboardEvent, index: number) {
  const count = props.tabs.length
  if (!count) return
  const rtl = (event.currentTarget as HTMLElement | null)?.closest('[dir]')?.getAttribute('dir') === 'rtl'
  const step = { ArrowRight: rtl ? -1 : 1, ArrowLeft: rtl ? 1 : -1 }[event.key]
  let next: number
  if (step !== undefined) next = (index + step + count) % count
  else if (event.key === 'Home') next = 0
  else if (event.key === 'End') next = count - 1
  else return
  event.preventDefault()
  const tab = props.tabs[next]
  if (!tab) return
  emit('update:modelValue', tab.key)
  void nextTick(() => list.value?.querySelectorAll<HTMLElement>('[role="tab"]')[next]?.focus())
}
</script>

<template>
  <div ref="list" class="tabs" role="tablist">
    <button
      v-for="(tab, index) in tabs"
      :key="tab.key"
      type="button"
      role="tab"
      class="tab pressable"
      :class="{ 'tab-active': modelValue === tab.key }"
      :aria-selected="modelValue === tab.key"
      :tabindex="focusable === tab.key ? 0 : -1"
      @click="emit('update:modelValue', tab.key)"
      @keydown="onKeydown($event, index)"
    >
      {{ tab.label }}
      <span
        v-if="tab.count !== undefined"
        class="ms-1 text-xs opacity-70"
      >{{ tab.count }}</span>
    </button>
  </div>
</template>
