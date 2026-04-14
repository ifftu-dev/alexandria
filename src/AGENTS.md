# alexandria/src/

**Generated:** 2026-04-15

## Standing Instructions

- **Documentation review after code changes**: After completing any code changes, always assess whether README and other docs need updating. Ask the user for permission before modifying any documentation files.

## Overview

Vue 3 SPA frontend for the Tauri app. 14 composables, 30 route views, and a 1332-line `types/index.ts`.

## WHERE TO LOOK

| Task | Location | Notes |
|------|----------|-------|
| State | `composables/` | 14 singleton refs (no Pinia/Vuex) |
| UI design system | `components/ui/` | 12 components, barrel-exported |
| Pages | `pages/` | 3 root pages + 9 feature dirs (`classrooms`, `courses`, `dashboard`, `governance`, `instructor`, `learn`, `opinions`, `skills`, `tutoring`) |
| Types | `types/index.ts` | All TS interfaces (mirrors Rust domain) |
| Styling | `assets/css/` | Tailwind v4 + CSS custom properties |
| Routing | `router/` | Vue Router config |
| ML/integrity | `utils/sentinel/` | Mouse/keystroke/face ML models for Sentinel |

## CONVENTIONS (TypeScript/Vue)

```typescript
// TypeScript strict mode — NEVER use:
any, @ts-ignore, @ts-expect-error

// Type imports (always)
import type { Course, Enrollment } from '@/types'

// Vue SFC order
<script setup lang="ts">
import { ref, computed } from 'vue'          // 1. Vue core
import { useRoute } from 'vue-router'         // 2. external
import { useAuth } from '@/composables/useAuth' // 3. composables
import { AppButton } from '@/components/ui'   // 4. components
import type { Props } from '@/types'          // 5. types

interface Props { title: string; count?: number }
withDefaults(defineProps<Props>(), { count: 0 })
defineEmits<{ change: [value: number] }>()
</script>

<template>
  <AppButton @click="$emit('change', count + 1)">{{ title }}</AppButton>
</template>

<style scoped>/* only if Tailwind insufficient */</style>
```

## STATE MANAGEMENT

```typescript
// composables/useItems.ts — module-level singleton
const items = ref<Item[]>([])

export function useItems() {
  return {
    items: readonly(items),
    async fetchItems() { /* ... */ }
  }
}
```

No Pinia/Vuex. All state as `readonly()` singletons.

## UI COMPONENTS (barrel export)

```typescript
// src/components/ui/index.ts exports:
AppButton, AppModal, AppInput, AppSpinner, AppAlert, 
AppBadge, AppTabs, AppTextarea, ConfirmDialog, 
DataRow, EmptyState, StatusBadge
```

## COMPLEXITY HOTSPOTS

| File | Lines | Role |
|------|-------|------|
| `composables/useSentinel.ts` | 903 | Sentinel monitoring + on-device ML |
| `types/index.ts` | 1332 | All TS domain interfaces |
| `utils/sentinel/mouse-trajectory-cnn.ts` | 413 | 90 functions — gesture classifier |
| `utils/sentinel/keystroke-autoencoder.ts` | 397 | 67 functions — keystroke ML |
| `utils/sentinel/face-embedder.ts` | 380 | 66 functions — face embedding |
| `pages/learn/Player.vue` | 1037 | Course player with video/quiz |
| `pages/tutoring/Session.vue` | 1137 | Live tutoring UI |
