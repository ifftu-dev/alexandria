import { beforeEach, expect, it, vi } from 'vitest'
import { createMemoryHistory, createRouter, type Router } from 'vue-router'

const mocks = vi.hoisted(() => ({ router: null as Router | null, profile: { value: 'A' }, invoke: vi.fn().mockResolvedValue(null) }))
vi.mock('@/router', () => ({ get default() { return mocks.router } }))
vi.mock('@/composables/useProfiles', () => ({ useProfiles: () => ({ isUnlocked: { value: true }, activeProfileId: mocks.profile }), onProfileReady: () => () => {} }))
vi.mock('@/composables/useLocalApi', () => ({ useLocalApi: () => ({ invoke: mocks.invoke }) }))

beforeEach(() => {
  vi.resetModules()
  mocks.profile.value = 'A'
  mocks.invoke.mockClear()
  mocks.router = createRouter({ history: createMemoryHistory(), routes: [
    { path: '/', component: { template: '<div />' } },
    { path: '/settings/organisations', component: { template: '<div />' } },
  ] })
})

it('cancels notification navigation if the session changes during router resolution', async () => {
  const router = mocks.router!
  await router.push('/')
  let release!: () => void
  let started!: () => void
  const entered = new Promise<void>(resolve => { started = resolve })
  router.beforeEach(async to => {
    if (to.path === '/settings/organisations') {
      started()
      await new Promise<void>(resolve => { release = resolve })
    }
  })
  const sessions = await import('@/composables/profileSession')
  sessions.setProfileSessionToken('session-A')
  const links = (await import('./useDeepLinks')).useDeepLinks()
  const opening = links.openNotification({ id: 'tap', recipient: 'r', profileId: 'A', networkId: 'n', route: '/settings/organisations' })
  await entered
  sessions.setProfileSessionToken('session-B')
  mocks.profile.value = 'B'
  release()
  await opening
  expect(router.currentRoute.value.path).toBe('/')
  expect(mocks.invoke).not.toHaveBeenCalled()
})
