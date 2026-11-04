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
      if (command === 'list_profiles') return new URLSearchParams(location.search).get('existing') === '1' ? [{ id: 'motion-test', display_name: 'Test learner', avatar: { kind: 'emoji', value: '🌱' }, color: '#6366f1', last_unlocked_at: null }] : []
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
if (screen === 'motion') {
  const { default: MotionFixture } = await import('./MotionFixture.vue')
  const router = createRouter({ history: createMemoryHistory(), routes: [{ path: '/:pathMatch(.*)*', component: MotionFixture }] })
  await router.push('/')
  createApp(MotionFixture).use(i18n).use(router).mount('#app')
} else if (screen === 'personal-atlas') {
  const { atlasBackend } = await import('./atlasBackend')
  const scenario = params.get('scenario')
  const backend = atlasBackend(scenario === 'stress' || scenario === 'scale' ? scenario : null)
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
    { path: '/courses', component: (await import('@/pages/courses/Index.vue')).default },
    { path: '/courses/:id', component: (await import('@/pages/courses/Detail.vue')).default },
    { path: '/skills', component: Skills }, { path: '/skills/:id', component: Detail }, { path: '/goals', component: Goals },
  ] })
  await router.push('/skills')
  createApp(PersonalAtlasFixture, { backend }).use(i18n).use(router).mount('#app')
} else if (screen === 'notification-links') {
  // The real router and deep-link runtime over an unlocked profile whose inbox
  // holds one unread row. Every IPC call is recorded for the spec to inspect.
  const calls: string[] = []
  const row = { id: 'n-1', kind: 'offer_sent', title: '', body: '', route: '/settings/organisations',
    source: 'local', created_at: '2026-11-04T00:00:00Z', read_at: null, payload: null }
  const answers: Record<string, unknown> = {
    get_incompatible_profile_data: null, get_profile_cleanup_required: false, list_profiles: [],
    get_active_profile_id: 'profile-a', get_profile_session_token: 'session-a',
    get_account_status: { account_roles: ['learner'], activation_state: 'active' },
    notifications_list: [row], notifications_unread_count: 1, 'plugin:event|listen': 1,
  }
  Object.defineProperty(window, '__TAURI_EVENT_PLUGIN_INTERNALS__', { value: { unregisterListener: () => {} } })
  Object.defineProperty(window, '__TAURI_INTERNALS__', { configurable: true, value: {
    transformCallback: () => 1, unregisterCallback: () => {},
    invoke: async (command: string) => {
      calls.push(command)
      return command in answers ? structuredClone(answers[command]) : null
    },
  } })
  const { default: router } = await import('@/router')
  const { useProfiles } = await import('@/composables/useProfiles')
  const { useDeepLinks } = await import('@/deeplink/useDeepLinks')
  await useProfiles().initialize()
  router.addRoute({ path: '/fixture-start', component: { render: () => h('p', 'Fixture start') } })
  await router.push('/fixture-start')
  createApp({ render: () => h(RouterView) }).use(i18n).use(router).mount('#app')
  Object.assign(window, { notificationLinks: {
    calls,
    path: () => router.currentRoute.value.fullPath,
    openLink: (url: string) => useDeepLinks().openLink(url),
  } })
} else if (screen === 'motion-surfaces') {
  const { motionBackend } = await import('./motionBackend')
  const backend = motionBackend()
  Object.defineProperty(window, '__TAURI_EVENT_PLUGIN_INTERNALS__', { value: { unregisterListener: () => {} } })
  Object.defineProperty(window, '__TAURI_INTERNALS__', { configurable: true, value: {
    invoke: backend.invoke, transformCallback: () => 1, unregisterCallback: () => {},
  } })
  const { useProfiles } = await import('@/composables/useProfiles')
  const { useSettings } = await import('@/composables/useSettings')
  const { useAccountStatus } = await import('@/composables/useAccountStatus')
  await useProfiles().initialize()
  await Promise.all([useSettings().initialize(), useAccountStatus().refreshAccountStatus()])
  const page = { render: () => h('p', 'Fixture page') }
  const router = createRouter({ history: createMemoryHistory(), routes: [
    { path: '/home', component: (await import('@/pages/Home.vue')).default },
    { path: '/goals', component: (await import('@/pages/goals/Index.vue')).default },
    { path: '/courses', component: (await import('@/pages/courses/Index.vue')).default },
    { path: '/learn/:id', component: (await import('@/pages/learn/Player.vue')).default },
    { path: '/settings/:section?', name: 'settings', component: (await import('@/pages/Settings.vue')).default },
    { path: '/chat', component: (await import('./ChatFixture.vue')).default },
    { path: '/tutoring/:id', name: 'tutoring-session', component: (await import('@/pages/tutoring/Session.vue')).default },
    { path: '/classrooms', component: (await import('@/pages/classrooms/Index.vue')).default },
    { path: '/classrooms/:id', name: 'classroom', component: (await import('@/pages/classrooms/Classroom.vue')).default },
    { path: '/classrooms/:id/requests', name: 'classroom-requests', component: page },
    { path: '/classrooms/:id/settings', name: 'classroom-settings', component: page },
    { path: '/guardian', component: (await import('@/pages/guardian/Children.vue')).default },
    { path: '/guardian/child/:linkId', component: (await import('@/pages/guardian/ChildDetail.vue')).default },
    { path: '/instructor/courses', component: (await import('@/pages/instructor/MyCourses.vue')).default },
    { path: '/instructor/composer/:id', component: (await import('@/pages/instructor/Composer.vue')).default },
    { path: '/:pathMatch(.*)*', component: page },
  ] })
  await router.push(params.get('path') ?? '/home')
  const { default: MotionSurfacesFixture } = await import('./MotionSurfacesFixture.vue')
  createApp(MotionSurfacesFixture).use(i18n).use(router).mount('#app')
  Object.assign(window, { motionFixture: { unexpected: backend.unexpected } })
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
