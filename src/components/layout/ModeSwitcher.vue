<script setup lang="ts">
import { useRouter } from 'vue-router'
import { useMode, type AppMode } from '@/composables/useMode'

const router = useRouter()
const { mode, canSwitchModes, setMode } = useMode()

async function select(next: AppMode) {
  if (next === mode.value) return
  await setMode(next)
  // Land on the natural home of the chosen surface.
  void router.push(next === 'instructor' ? '/instructor' : '/home')
}
</script>

<template>
  <div
    v-if="canSwitchModes"
    class="mode-switcher"
    role="radiogroup"
    aria-label="Active mode"
  >
    <button
      class="mode-switcher-btn"
      :class="{ 'mode-switcher-btn--active-learner': mode === 'learner' }"
      role="radio"
      :aria-checked="mode === 'learner'"
      title="Learner mode — take courses and earn credentials"
      @click="select('learner')"
    >
      <svg class="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
        <path stroke-linecap="round" stroke-linejoin="round" d="M12 6.253v13m0-13C10.832 5.477 9.246 5 7.5 5S4.168 5.477 3 6.253v13C4.168 18.477 5.754 18 7.5 18s3.332.477 4.5 1.253m0-13C13.168 5.477 14.754 5 16.5 5c1.747 0 3.332.477 4.5 1.253v13C19.832 18.477 18.247 18 16.5 18c-1.746 0-3.332.477-4.5 1.253" />
      </svg>
      Learner
    </button>
    <button
      class="mode-switcher-btn"
      :class="{ 'mode-switcher-btn--active-instructor': mode === 'instructor' }"
      role="radio"
      :aria-checked="mode === 'instructor'"
      title="Instructor mode — compose courses and review submissions"
      @click="select('instructor')"
    >
      <svg class="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
        <path stroke-linecap="round" stroke-linejoin="round" d="M9.53 16.122a3 3 0 00-5.78 1.128 2.25 2.25 0 01-2.4 2.245 4.5 4.5 0 008.4-2.245c0-.399-.078-.78-.22-1.128zm0 0a15.998 15.998 0 003.388-1.62m-5.043-.025a15.994 15.994 0 011.622-3.395m3.42 3.42a15.995 15.995 0 004.764-4.648l3.876-5.814a1.151 1.151 0 00-1.597-1.597L14.146 6.32a15.996 15.996 0 00-4.649 4.763m3.42 3.42a6.776 6.776 0 00-3.42-3.42" />
      </svg>
      Instructor
    </button>
  </div>
</template>

<style scoped>
.mode-switcher {
  display: flex;
  align-items: center;
  padding: 0.125rem;
  gap: 0.125rem;
  border-radius: 0.5rem;
  background: color-mix(in srgb, var(--app-muted) 45%, transparent);
  border: 1px solid color-mix(in srgb, var(--app-border) 60%, transparent);
}

.mode-switcher-btn {
  display: flex;
  align-items: center;
  gap: 0.3125rem;
  padding: 0.25rem 0.625rem;
  border: none;
  border-radius: 0.375rem;
  background: transparent;
  font-size: 0.75rem;
  font-weight: 500;
  color: var(--app-muted-foreground);
  cursor: pointer;
  transition: color 0.15s, background 0.15s, box-shadow 0.15s;
}

.mode-switcher-btn:hover {
  color: var(--app-foreground);
}

.mode-switcher-btn--active-learner {
  background: var(--app-card);
  color: var(--app-primary);
  box-shadow: 0 1px 2px rgb(0 0 0 / 0.08);
}

.mode-switcher-btn--active-instructor {
  background: var(--app-card);
  color: var(--mode-instructor-accent, #b45309);
  box-shadow: 0 1px 2px rgb(0 0 0 / 0.08);
}
</style>
