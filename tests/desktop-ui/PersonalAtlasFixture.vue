<script setup lang="ts">
import { computed } from 'vue'
import { RouterView, useRoute, useRouter } from 'vue-router'
import { useProfiles } from '@/composables/useProfiles'
import { announceCredentialsChanged } from '@/composables/credentialEvents'
import { clearSettingsCache } from '@/composables/useSettings'
import SkillAtlasHost from '@/components/skills/SkillAtlasHost.vue'
import SidebarSkillGraph from '@/components/layout/SidebarSkillGraph.vue'
import LocaleDropdown from '@/components/settings/LocaleDropdown.vue'
import type { atlasBackend } from './atlasBackend'

const props = defineProps<{ backend: ReturnType<typeof atlasBackend> }>()
const profiles = useProfiles()
const router = useRouter()
const route = useRoute()
const unlocked = computed(() => profiles.isUnlocked.value && !profiles.isLockBlocked.value)
function improve(): void { props.backend.state.level = 4; announceCredentialsChanged() }
async function lock(): Promise<void> { await profiles.lockProfile(); clearSettingsCache() }
async function unlock(): Promise<void> { props.backend.state.earned = false; await profiles.unlockProfile('profile-b', 'fixture-only'); await router.push('/skills') }
</script>

<template>
  <SkillAtlasHost />
  <main class="personal-fixture">
    <header class="fixture-controls">
      <span>Personal surfaces · full bootstrap taxonomy · strict IPC</span>
      <button @click="improve">Simulate level update</button>
      <button @click="router.back()">History back</button>
      <button v-if="unlocked" @click="lock">Lock profile</button><button v-else @click="unlock">Unlock another profile</button>
      <LocaleDropdown />
      <output>{{ route.fullPath }}</output>
    </header>
    <div v-if="unlocked" class="personal-layout">
      <aside class="fixture-sidebar"><SidebarSkillGraph /></aside>
      <section class="fixture-page"><RouterView /></section>
    </div>
    <p v-else>Profile locked</p>
  </main>
</template>

<style scoped>
.personal-fixture { height: 100%; overflow: auto; padding: 24px; }
.fixture-controls { display: flex; gap: 12px; flex-wrap: wrap; align-items: center; margin-block-end: 24px; font-size: 12px; }
.fixture-controls > button { border: 1px solid var(--app-border); border-radius: 8px; padding: 10px; min-height: 44px; }
.personal-layout { display: grid; grid-template-columns: 240px minmax(0, 1fr); gap: 24px; }
.fixture-sidebar { padding: 12px; border: 1px solid var(--app-border); border-radius: 12px; align-self: start; }
.fixture-page { min-width: 0; }
@media (max-width: 700px) { .personal-fixture { padding: 12px; } .personal-layout { grid-template-columns: minmax(0, 1fr); } }
</style>
