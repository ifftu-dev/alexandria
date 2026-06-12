<script setup lang="ts">
import { computed } from 'vue'
import type { PublicProfile } from '@/types'

const props = defineProps<{
  profile: PublicProfile
  /** Viewing your own profile — shows visibility + edit affordances. */
  isOwn?: boolean
  /** Own profile only: "public" | "private". */
  visibility?: string | null
}>()

defineEmits<{ edit: [] }>()

const name = computed(
  () => props.profile.display_name || props.profile.username || 'Unnamed',
)
const initial = computed(() => (name.value[0] ?? '?').toUpperCase())

// Deterministic banner hue from the DID so every profile gets its own
// color identity without storing anything.
const hue = computed(() => {
  let h = 0
  for (const c of props.profile.did) h = (h * 31 + c.charCodeAt(0)) % 360
  return h
})

const copied = defineModel<boolean>('copied', { default: false })
async function copyDid() {
  try {
    await navigator.clipboard.writeText(props.profile.did)
    copied.value = true
    setTimeout(() => (copied.value = false), 1500)
  } catch {
    // Clipboard unavailable (e.g. webview restrictions) — ignore.
  }
}
</script>

<template>
  <div class="ph-card">
    <!-- Banner -->
    <div
      class="ph-banner"
      :style="{
        background: `linear-gradient(120deg,
          hsl(${hue} 70% 62%) 0%,
          hsl(${(hue + 40) % 360} 65% 55%) 55%,
          hsl(${(hue + 90) % 360} 60% 48%) 100%)`,
      }"
    />

    <!-- Avatar + identity -->
    <div class="px-5 pb-5">
      <div class="-mt-9 mb-3 flex items-end justify-between">
        <div
          class="ph-avatar"
          :style="{ background: `hsl(${hue} 65% 45%)` }"
        >
          {{ initial }}
        </div>
        <div class="flex items-center gap-2">
          <span v-if="isOwn && visibility === 'private'" class="ph-vis ph-vis--private">
            🔒 Private
          </span>
          <span v-else-if="isOwn" class="ph-vis ph-vis--public">
            🌐 Public
          </span>
          <slot name="actions" />
        </div>
      </div>

      <h1 class="ph-name">{{ name }}</h1>
      <p v-if="profile.username" class="mt-0.5 text-sm font-medium text-primary">
        @{{ profile.username }}
      </p>

      <p v-if="profile.bio" class="mt-3 max-w-prose text-sm text-foreground/85">
        {{ profile.bio }}
      </p>

      <button
        class="mt-3 inline-flex items-center gap-1 text-[0.65rem] text-muted-foreground hover:text-foreground"
        :title="profile.did"
        @click="copyDid"
      >
        <span class="max-w-[16rem] truncate font-mono">{{ profile.did }}</span>
        <span>{{ copied ? '✓ copied' : '⧉' }}</span>
      </button>
    </div>
  </div>
</template>

<style scoped>
.ph-card {
  border-radius: 1rem;
  background: var(--app-card);
  box-shadow: 0 1px 3px rgb(0 0 0 / 8%);
  overflow: hidden;
}
.ph-banner {
  height: 5.5rem;
}
.ph-avatar {
  width: 4.5rem;
  height: 4.5rem;
  border-radius: 9999px;
  border: 3px solid var(--app-card);
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 1.6rem;
  font-weight: 700;
  color: white;
}
.ph-name {
  font-family: 'Libre Baskerville', 'DM Serif Display', Georgia, serif;
  font-size: 1.45rem;
  line-height: 1.25;
  color: var(--app-foreground);
}
.ph-vis {
  font-size: 0.68rem;
  font-weight: 600;
  padding: 0.2rem 0.6rem;
  border-radius: 999px;
}
.ph-vis--public {
  background: color-mix(in srgb, var(--app-success) 15%, transparent);
  color: var(--app-success);
}
.ph-vis--private {
  background: color-mix(in srgb, var(--app-warning) 18%, transparent);
  color: var(--app-warning);
}
</style>
