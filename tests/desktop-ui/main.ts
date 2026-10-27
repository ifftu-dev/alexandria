import { createApp, h } from 'vue'
import { createMemoryHistory, createRouter, RouterView } from 'vue-router'
import { i18n, setActiveLocale } from '@/i18n'
import { loadLocaleMessages } from '@/i18n/loadLocale'
import { directionOf, isAppLocale } from '@/locales/meta'
import '@/assets/css/main.css'
import type { GoalTemplate, P2PStatus, PersonhoodLabStatus } from '@/types'

const templates: GoalTemplate[] = [{
  id: 'audit-software-engineer',
  kind: 'job_role',
  key: 'software-engineer',
  label: 'Software engineer',
  skill_ids: ['test-skill'],
  ratified: true,
}]
const offlineNetwork: P2PStatus = {
  is_running: false, peer_id: null, listening_addresses: [], connected_peers: 0,
  subscribed_topics: [], nat_status: 'unknown', relay_addresses: [],
}
const disabledLab: PersonhoodLabStatus = {
  enabled: false, phase: 'idle', key_status: 'missing', downloaded_bytes: 0,
  total_bytes: 0, elapsed_ms: 0, error: null, result: null,
}

Object.defineProperty(window, '__TAURI_INTERNALS__', {
  configurable: true,
  value: {
    transformCallback: () => 1,
    invoke: async (command: string) => {
      if (command === 'list_goal_templates') return templates
      if (command === 'list_profiles') return []
      if (command === 'personhood_lab_status') return disabledLab
      if (command === 'plugin:event|listen') return 1
      if (command === 'plugin:biometry|status') return { isAvailable: false }
      if (['list_courses', 'list_enrollments', 'list_settings', 'get_reputation'].includes(command)) return []
      if (['get_my_skill_graph', 'get_local_did', 'check_my_username_conflict'].includes(command)) return null
      if (command === 'hydrate_catalog_courses') return 0
      if (command === 'p2p_status') return offlineNetwork
      if (['p2p_start', 'claim_username'].includes(command)) return undefined
      throw new Error(`Unexpected fixture IPC command: ${command}`)
    },
  },
})

const params = new URLSearchParams(location.search)
document.documentElement.classList.toggle('dark', params.get('theme') === 'dark')
const requestedLocale = params.get('locale')
if (isAppLocale(requestedLocale)) {
  await loadLocaleMessages(requestedLocale)
  setActiveLocale(requestedLocale)
  document.documentElement.lang = requestedLocale
  document.documentElement.dir = directionOf(requestedLocale)
}
const screen = params.get('screen')
if (screen === 'personal-atlas') {
  const { atlasBackend } = await import('./atlasBackend')
  const backend = atlasBackend(params.get('scenario') === 'stress')
  Object.defineProperty(window, '__TAURI_EVENT_PLUGIN_INTERNALS__', { value: { unregisterListener: () => {} } })
  Object.defineProperty(window, '__TAURI_INTERNALS__', { configurable: true, value: {
    invoke: backend.invoke, transformCallback: () => 1, unregisterCallback: () => {},
  } })
  const { default: PersonalAtlasFixture } = await import('./PersonalAtlasFixture.vue')
  const { default: Skills } = await import('@/pages/skills/Index.vue')
  const { default: Detail } = await import('@/pages/skills/Detail.vue')
  const { default: Goals } = await import('@/pages/goals/Index.vue')
  const { useProfiles } = await import('@/composables/useProfiles')
  await useProfiles().initialize()
  const router = createRouter({ history: createMemoryHistory(), routes: [
    { path: '/skills', component: Skills }, { path: '/skills/:id', component: Detail }, { path: '/goals', component: Goals },
  ] })
  await router.push('/skills')
  createApp(PersonalAtlasFixture, { backend }).use(i18n).use(router).mount('#app')
} else if (screen === 'skill-atlas') {
  // Keep the props-only fixture free of unrelated page lifecycle and global CSS.
  const { default: SkillAtlasFixture } = await import('./SkillAtlasFixture.vue')
  createApp(SkillAtlasFixture).use(i18n).mount('#app')
} else {
  const { default: Fixture } = await import('./Fixture.vue')
  const { default: Onboarding } = await import('@/pages/Onboarding.vue')
  const { default: BlankLayout } = await import('@/layouts/BlankLayout.vue')
  const { default: Home } = await import('@/pages/Home.vue')
  const { default: Goals } = await import('@/pages/goals/Index.vue')
  const router = createRouter({ history: createMemoryHistory(), routes: [
    { path: '/', component: Home },
    { path: '/goals', component: Goals },
    { path: '/:pathMatch(.*)*', component: Fixture },
  ] })
  const root = screen === 'onboarding'
    ? { render: () => h(BlankLayout, null, { default: () => h(Onboarding) }) }
    : screen === 'home'
    ? { render: () => h(BlankLayout, null, { default: () => h('div', { class: 'p-6' }, [h(RouterView)]) }) }
    : Fixture
  createApp(root).use(i18n).use(router).mount('#app')
}
