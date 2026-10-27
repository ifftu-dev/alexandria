<script setup lang="ts">
withDefaults(defineProps<{ opening?: boolean }>(), { opening: false })
</script>

<template>
  <svg class="brand-mark" :class="{ 'brand-mark--opening': opening }" viewBox="0 0 64 64" fill="none" aria-hidden="true">
    <g class="brand-mark__points" fill="currentColor">
      <circle cx="32" cy="6" r="2.5" />
      <circle cx="8" cy="18" r="2.5" />
      <circle cx="56" cy="18" r="2.5" />
      <circle cx="8" cy="46" r="2.5" />
      <circle cx="56" cy="46" r="2.5" />
      <circle cx="32" cy="58" r="2.5" />
    </g>
    <path class="brand-mark__outline" pathLength="1" d="M32 6 8 18v28l24 12 24-12V18L32 6Z" stroke="currentColor" stroke-width="3" stroke-linejoin="round" />
    <path class="brand-mark__branches" pathLength="1" d="M32 18v28M16 26l16 8 16-8" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round" />
  </svg>
</template>

<style scoped>
.brand-mark { display: block; width: 100%; height: 100%; overflow: visible; color: var(--app-primary); }
.brand-mark__points { opacity: 0; }
.brand-mark--opening .brand-mark__points { transform-origin: center; animation: mark-points 680ms var(--ease-settle) both; }
.brand-mark--opening .brand-mark__outline { animation: mark-draw 680ms var(--ease-settle) both; }
.brand-mark--opening .brand-mark__branches { transform-origin: center; animation: mark-branches 800ms var(--ease-settle) both; }
@keyframes mark-points {
  0% { opacity: 0; transform: scale(1.65) rotate(-12deg); }
  30%, 55% { opacity: 1; transform: scale(1); }
  100% { opacity: 0; transform: scale(1); }
}
@keyframes mark-draw {
  0%, 15% { stroke-dasharray: 1; stroke-dashoffset: 1; opacity: 0; }
  25% { opacity: 1; }
  100% { stroke-dasharray: 1; stroke-dashoffset: 0; opacity: 1; }
}
@keyframes mark-branches {
  0%, 35% { opacity: 0; transform: scaleY(0.3); }
  100% { opacity: 1; transform: scaleY(1); }
}
</style>
