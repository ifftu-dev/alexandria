<script setup lang="ts">
// Renders a small "AI-generated example" pill when the bound piece
// of content (course, tutorial, opinion) was inserted by the dev
// seed. Shows nothing for user-created content.
//
// The `provenance` field arrives from the Rust IPC layer and reflects
// the value of `courses.provenance` / `opinions.provenance`. Currently
// only `'ai_generated'` is rendered — other values fall through and
// the badge stays hidden.

interface Props {
  provenance?: string | null
}

const props = defineProps<Props>()

const isAiGenerated = () => props.provenance === 'ai_generated'
</script>

<template>
  <span v-if="isAiGenerated()" class="provenance-badge" title="This is AI-generated example content seeded for the demo. It is not a real course / tutorial / opinion.">
    <svg class="provenance-badge__icon" viewBox="0 0 16 16" width="12" height="12" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
      <path d="M8 1.5 L9.4 5.6 L13.5 7 L9.4 8.4 L8 12.5 L6.6 8.4 L2.5 7 L6.6 5.6 Z" />
      <circle cx="13" cy="2.5" r="0.8" />
      <circle cx="2.5" cy="12.5" r="0.6" />
    </svg>
    <span>AI-generated example</span>
  </span>
</template>

<style scoped>
.provenance-badge {
  display: inline-flex;
  align-items: center;
  gap: 0.35rem;
  padding: 0.18rem 0.55rem;
  font-size: 0.7rem;
  font-weight: 500;
  letter-spacing: 0.01em;
  color: var(--color-amber-700, #b45309);
  background: var(--color-amber-50, rgba(245, 158, 11, 0.08));
  border: 1px solid var(--color-amber-200, rgba(245, 158, 11, 0.25));
  border-radius: 9999px;
  white-space: nowrap;
  line-height: 1;
}

.provenance-badge__icon {
  flex-shrink: 0;
}

@media (prefers-color-scheme: dark) {
  .provenance-badge {
    color: var(--color-amber-300, #fcd34d);
    background: var(--color-amber-900, rgba(245, 158, 11, 0.12));
    border-color: var(--color-amber-700, rgba(245, 158, 11, 0.3));
  }
}
</style>
