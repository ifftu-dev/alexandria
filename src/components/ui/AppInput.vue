<script setup lang="ts">
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
</script>

<template>
  <div>
    <label v-if="label" class="label text-xs text-[rgb(var(--color-muted-foreground))]">
      {{ label }}
    </label>
    <input
      :value="modelValue"
      :type="type"
      :placeholder="placeholder"
      :disabled="disabled"
      class="input"
      :class="{ 'input-error': error }"
      @input="$emit('update:modelValue', ($event.target as HTMLInputElement).value)"
    >
    <p v-if="error" class="text-xs text-[rgb(var(--color-error))] mt-1">{{ error }}</p>
  </div>
</template>
