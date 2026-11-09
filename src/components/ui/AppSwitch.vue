<script setup lang="ts">
// An on/off control that takes effect at once, as opposed to a checkbox that
// waits for a Save button. The label is the caller's: a switch whose meaning
// lives only in its position is unreadable to a screen reader. The switch
// takes the shared press treatment; its thumb slides over the base token, and
// in Reduced and Off it moves at once.
interface Props {
  modelValue: boolean
  disabled?: boolean
  label?: string
}

withDefaults(defineProps<Props>(), {
  disabled: false,
  label: '',
})

defineEmits<{ 'update:modelValue': [value: boolean] }>()
</script>

<template>
  <button
    type="button"
    role="switch"
    :aria-checked="modelValue"
    :aria-label="label || undefined"
    :disabled="disabled"
    class="pressable relative inline-flex h-6 w-11 shrink-0 cursor-pointer rounded-full border-2 border-transparent focus:outline-none focus-visible:ring-2 focus-visible:ring-primary focus-visible:ring-offset-2 disabled:cursor-not-allowed disabled:opacity-50"
    :class="modelValue ? 'bg-primary' : 'bg-muted-foreground/30'"
    @click="$emit('update:modelValue', !modelValue)"
  >
    <span
      class="app-switch-thumb pointer-events-none inline-block h-5 w-5 rounded-full bg-white shadow ring-0"
      :class="modelValue ? 'translate-x-5 rtl:-translate-x-5' : 'translate-x-0'"
    />
  </button>
</template>

<style scoped>
.app-switch-thumb { transition: translate var(--transition-base); }
</style>
