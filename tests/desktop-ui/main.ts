import { createApp } from 'vue'
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
    invoke: async (command: string) => {
      if (command === 'list_goal_templates') return templates
      throw new Error(`Unexpected fixture IPC command: ${command}`)
    },
  },
})

document.documentElement.classList.toggle('dark', new URLSearchParams(location.search).get('theme') === 'dark')
const { default: Fixture } = await import('./Fixture.vue')
const router = createRouter({ history: createMemoryHistory(), routes: [{ path: '/:pathMatch(.*)*', component: Fixture }] })
createApp(Fixture).use(i18n).use(router).mount('#app')
