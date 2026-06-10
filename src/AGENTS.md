# alexandria/src/

**Generated:** 2026-04-15

## Standing Instructions

- **Documentation review after code changes**: After completing any code changes, always assess whether README and other docs need updating. Ask the user for permission before modifying any documentation files.

## Overview

Vue 3 SPA frontend for the Tauri app. Module-level singleton composables (no Pinia/Vuex), 30+ route views, and a 1332-line `types/index.ts`. Supports multiple user profiles on one device — the picker (`/profiles`, `pages/ProfileSelect.vue`) is the first screen on launch when at least one profile exists.

## WHERE TO LOOK

| Task | Location | Notes |
|------|----------|-------|
| Multi-user state | `composables/useProfiles.ts` | Canonical surface: profiles, activeProfile, unlock/lock/create/rename/delete/setAvatar. Also exports `onProfileReady` / `onProfileLocked` fan-out hooks. |
| Per-profile settings | `composables/useSettings.ts` | Reactive mirror of the backend registry; `useSetting<T>('ui.theme')` two-way ref. Settings auto-sync across the user's devices (scope=`sync`) or stay local (scope=`device`). See [`docs/settings.md`](../docs/settings.md). |
| Auth compat shim | `composables/useAuth.ts` | Thin wrapper over `useProfiles` — kept so legacy components compile; removed lifecycle methods throw |
| State | `composables/` | Module-level singleton refs (no Pinia/Vuex) |
| UI design system | `components/ui/` | Barrel-exported primitives |
| Profile picker UI | `components/profile/` | `ProfileTile`, `AddProfileTile`, `ProfileAvatar` |
| Pages | `pages/` | Root pages (`ProfileSelect`, `Onboarding`, `Home`) + 11 feature dirs (`classrooms`, `courses`, `dashboard`, `governance`, `instructor`, `learn`, `opinions`, `skills`, `targets`, `tutoring`, `u` — instructor public graphs) |
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
import { useProfiles } from '@/composables/useProfiles' // 3. composables (useAuth is a back-compat shim)
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
| `composables/useSentinel.ts` | ~1.4k | Sentinel monitoring — event buffers + IPC dispatch to backend ML |
| `types/index.ts` | ~1.4k | All TS domain interfaces |
| `utils/sentinel/face-embedder.ts` | 380 | LBP face embedder (pure pixel math, only TS ML left after backend rewrite) |
| `../src-tauri/src/sentinel/paste_classifier.rs` | ~420 | tract ONNX paste classifier (backend) |
| `../src-tauri/src/sentinel/keystroke_ae.rs` | ~440 | candle autoencoder (backend) |
| `../src-tauri/src/sentinel/mouse_cnn.rs` | ~390 | candle dense-head CNN (backend) |
| `../src-tauri/src/sentinel/features.rs` | ~210 | 12-dim feature extractor (backend) |
| `pages/learn/Player.vue` | 1037 | Course player with video/quiz |
| `pages/tutoring/Session.vue` | 1137 | Live tutoring UI |
