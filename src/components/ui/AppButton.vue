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
    <!-- The label stays in the accessibility tree while loading, so the button
         keeps its name. With motion Off it also stays visible beside a still
         spinner (main.css, data-progress). -->
    <span v-if="loading" aria-hidden="true" data-progress class="spinner absolute" :style="{ width: '1em', height: '1em', borderWidth: '1.5px' }" />
    <span class="btn-label inline-flex items-center justify-center gap-2"><slot /></span>
  </button>
</template>
