import { createApp, h } from 'vue'
import { createMemoryHistory, createRouter } from 'vue-router'
import { i18n } from '@/i18n'
import '@/assets/css/main.css'
import type { GoalTemplate } from '@/types'

const templates: GoalTemplate[] = [{
  id: 'audit-software-engineer',
  kind: 'job_role',
  key: 'software-engineer',
  label: 'Software engineer',
  skill_ids: ['test-skill'],
  ratified: true,
}]

Object.defineProperty(window, '__TAURI_INTERNALS__', {
  value: {
    transformCallback: () => 1,
    invoke: async (command: string) => {
      if (command === 'list_goal_templates') return templates
      if (command === 'list_profiles') return []
      if (command === 'personhood_lab_status') return { enabled: false }
      if (command === 'plugin:event|listen') return 1
      if (command === 'plugin:biometry|status') return { isAvailable: false }
      throw new Error(`Unexpected fixture IPC command: ${command}`)
    },
  },
})

document.documentElement.classList.toggle('dark', new URLSearchParams(location.search).get('theme') === 'dark')
const { default: Fixture } = await import('./Fixture.vue')
const { default: Onboarding } = await import('@/pages/Onboarding.vue')
const { default: BlankLayout } = await import('@/layouts/BlankLayout.vue')
const router = createRouter({ history: createMemoryHistory(), routes: [{ path: '/:pathMatch(.*)*', component: Fixture }] })
const root = new URLSearchParams(location.search).get('screen') === 'onboarding'
  ? { render: () => h(BlankLayout, null, { default: () => h(Onboarding) }) }
  : Fixture
createApp(root).use(i18n).use(router).mount('#app')
