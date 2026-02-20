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
    class="btn"
    :class="[
      `btn-${variant}`,
      size !== 'md' ? `btn-${size}` : '',
      icon ? 'btn-icon' : '',
    ]"
    :disabled="disabled || loading"
  >
    <span v-if="loading" class="spinner" style="width: 1em; height: 1em; border-width: 1.5px;" />
    <slot v-else />
  </button>
</template>
