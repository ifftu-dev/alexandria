<script setup lang="ts">
import { ref } from 'vue'

interface Props {
  modelValue: string
  label?: string
  placeholder?: string
  type?: string
  error?: string
  disabled?: boolean
}

withDefaults(defineProps<Props>(), {
  label: '',
  placeholder: '',
  type: 'text',
  error: '',
  disabled: false,
})

defineEmits<{
  'update:modelValue': [value: string]
}>()

const inputEl = ref<HTMLInputElement | null>(null)

function focus() {
  inputEl.value?.focus()
}
function select() {
  inputEl.value?.select()
}
function blur() {
  inputEl.value?.blur()
}

defineExpose({ focus, select, blur })
</script>

<template>
  <div>
    <label v-if="label" class="label text-xs text-muted-foreground">
      {{ label }}
    </label>
    <input
      ref="inputEl"
      :value="modelValue"
      :type="type"
      :placeholder="placeholder"
      :disabled="disabled"
      class="input"
      :class="{ 'input-error': error }"
      @input="$emit('update:modelValue', ($event.target as HTMLInputElement).value)"
    >
    <p v-if="error" class="text-xs text-error mt-1">{{ error }}</p>
  </div>
</template>
