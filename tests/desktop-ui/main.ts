import { createApp, h } from 'vue'
import { createMemoryHistory, createRouter, RouterView } from 'vue-router'
import { i18n } from '@/i18n'
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

document.documentElement.classList.toggle('dark', new URLSearchParams(location.search).get('theme') === 'dark')
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
const screen = new URLSearchParams(location.search).get('screen')
const root = screen === 'onboarding'
  ? { render: () => h(BlankLayout, null, { default: () => h(Onboarding) }) }
  : screen === 'home'
  ? { render: () => h(BlankLayout, null, { default: () => h('div', { class: 'p-6' }, [h(RouterView)]) }) }
  : Fixture
createApp(root).use(i18n).use(router).mount('#app')
