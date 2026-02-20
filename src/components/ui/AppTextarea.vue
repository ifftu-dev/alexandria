<script setup lang="ts">
interface Props {
  modelValue: string
  label?: string
  placeholder?: string
  rows?: number
  error?: string
  disabled?: boolean
}

withDefaults(defineProps<Props>(), {
  label: '',
  placeholder: '',
  rows: 3,
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
    <textarea
      :value="modelValue"
      :placeholder="placeholder"
      :rows="rows"
      :disabled="disabled"
      class="input resize-none"
      :class="{ 'input-error': error }"
      @input="$emit('update:modelValue', ($event.target as HTMLTextAreaElement).value)"
    />
    <p v-if="error" class="text-xs text-[rgb(var(--color-error))] mt-1">{{ error }}</p>
  </div>
</template>
