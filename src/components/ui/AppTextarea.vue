<script setup lang="ts">
import { useId } from 'vue'
const generatedId = useId()

interface Props {
  id?: string
  maxlength?: number
  required?: boolean
  autocomplete?: string
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
    <label v-if="label" :for="id ?? generatedId" class="label text-xs text-muted-foreground">
      {{ label }}
    </label>
    <textarea
      :id="id ?? generatedId"
      :maxlength="maxlength"
      :required="required"
      :autocomplete="autocomplete"
      :aria-invalid="error ? true : undefined"
      :value="modelValue"
      :placeholder="placeholder"
      :rows="rows"
      :disabled="disabled"
      class="input resize-none"
      :class="{ 'input-error': error }"
      @input="$emit('update:modelValue', ($event.target as HTMLTextAreaElement).value)"
    />
    <p v-if="error" class="text-xs text-error mt-1">{{ error }}</p>
  </div>
</template>
