# alexandria/src/

**Generated:** 2026-03-20

## Overview

Vue 3 SPA frontend for the Tauri app. 13 composables, 10+ page directories, 965 TypeScript types.

## WHERE TO LOOK

| Task | Location | Notes |
|------|----------|-------|
| State | `composables/` | 13 singleton refs (no Pinia/Vuex) |
| UI design system | `components/ui/` | 12 components, barrel-exported |
| Pages | `pages/` | Home, Onboarding, Unlock + 7 feature dirs |
| Types | `types/index.ts` | All TS interfaces (mirrors Rust domain) |
| Styling | `assets/css/` | Tailwind v4 + CSS custom properties |
| Routing | `router/` | Vue Router config |
| ML/biometric | `utils/sentinel/` | Mouse/keystroke/face ML models |

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
| `composables/useSentinel.ts` | 903 | 165 functions — biometric auth ML |
| `types/index.ts` | 965 | All TS domain interfaces |
| `utils/sentinel/mouse-trajectory-cnn.ts` | 413 | 90 functions — gesture classifier |
| `utils/sentinel/keystroke-autoencoder.ts` | 397 | 67 functions — keystroke ML |
| `utils/sentinel/face-embedder.ts` | 380 | 66 functions — face embedding |
| `pages/learn/Player.vue` | 1037 | Course player with video/quiz |
| `pages/tutoring/Session.vue` | 1004 | Live tutoring UI |
