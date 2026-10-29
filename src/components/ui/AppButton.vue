<script setup lang="ts">
interface Props {
  variant?: 'primary' | 'secondary' | 'outline' | 'ghost' | 'danger' | 'governance'
  size?: 'xs' | 'sm' | 'md' | 'lg'
  loading?: boolean
  disabled?: boolean
  icon?: boolean
}

withDefaults(defineProps<Props>(), {
  variant: 'primary',
  size: 'md',
  loading: false,
  disabled: false,
  icon: false,
})
</script>

<template>
  <button
    class="btn relative"
    :class="[
      `btn-${variant}`,
      size !== 'md' ? `btn-${size}` : '',
      icon ? 'btn-icon' : '',
    ]"
    :disabled="disabled || loading"
    :aria-busy="loading || undefined"
  >
    <span v-if="loading" aria-hidden="true" class="spinner absolute" :style="{ width: '1em', height: '1em', borderWidth: '1.5px' }" />
    <span class="inline-flex items-center justify-center gap-2" :class="{ 'opacity-0': loading }"><slot /></span>
  </button>
</template>
