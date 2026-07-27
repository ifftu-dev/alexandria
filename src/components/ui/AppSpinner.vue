<script setup lang="ts">
interface Props {
  size?: 'xs' | 'sm' | 'md' | 'lg'
  label?: string
}

withDefaults(defineProps<Props>(), {
  size: 'md',
  label: '',
})

// Dimensions come from this prop, never from a caller's class: a `class` on
// this component lands on the wrapper below, which would leave the spinner
// itself at its default size and overflowing.
const sizeMap = {
  xs: 'w-3.5 h-3.5',
  sm: 'w-4 h-4',
  md: 'w-6 h-6',
  lg: 'w-8 h-8',
}
</script>

<template>
  <div class="flex items-center gap-2">
    <!-- Stroked SVG arc rather than a bordered div. A border-radius ring
         quantises its border to whole pixels, so at small sizes the
         transparent segment that shows rotation turns into a hard notch and
         the ring reads as a broken shape. A stroked circle stays smooth at any
         size and lets the moving arc have round caps. -->
    <svg
      class="app-spinner"
      :class="sizeMap[size]"
      viewBox="0 0 24 24"
      fill="none"
      role="status"
      :aria-label="label || undefined"
    >
      <circle class="app-spinner__track" cx="12" cy="12" r="9.5" stroke-width="2.5" />
      <circle
        class="app-spinner__arc"
        cx="12"
        cy="12"
        r="9.5"
        stroke-width="2.5"
        stroke-linecap="round"
      />
    </svg>
    <span v-if="label" class="text-sm text-muted-foreground">{{ label }}</span>
  </div>
</template>

<style scoped>
.app-spinner {
  flex: none;
  color: var(--app-primary);
  animation: app-spinner-rotate 1.4s linear infinite;
}
.app-spinner__track {
  stroke: currentColor;
  opacity: 0.18;
}
.app-spinner__arc {
  stroke: currentColor;
  /* Circumference is 2πr ≈ 59.7. The dash pair grows and shrinks against that
     length so the arc sweeps rather than rigidly rotating a fixed gap. */
  stroke-dasharray: 6 60;
  animation: app-spinner-dash 1.4s ease-in-out infinite;
}

@keyframes app-spinner-rotate {
  to {
    transform: rotate(360deg);
  }
}
@keyframes app-spinner-dash {
  0% {
    stroke-dasharray: 6 60;
    stroke-dashoffset: 0;
  }
  50% {
    stroke-dasharray: 40 60;
    stroke-dashoffset: -14;
  }
  100% {
    stroke-dasharray: 6 60;
    stroke-dashoffset: -52;
  }
}

/* Still show motion for reduced-motion users, just slower and without the
   sweeping arc — a static gap rotating gently. */
@media (prefers-reduced-motion: reduce) {
  .app-spinner {
    animation-duration: 3s;
  }
  .app-spinner__arc {
    animation: none;
    stroke-dasharray: 30 60;
  }
}
</style>
