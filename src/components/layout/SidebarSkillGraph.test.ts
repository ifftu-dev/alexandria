import { flushPromises, mount } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { ref } from 'vue'

const state = vi.hoisted(() => ({
  creds: [] as unknown[],
  listCalls: 0,
  navigate: [] as Array<() => void>,
}))

vi.mock('@/composables/useLocalApi', () => ({
  useLocalApi: () => ({
    invoke: async (command: string) => {
      if (command === 'list_skills') return [{ id: 'skill_big_o', name: 'Big-O', bloom_level: 'remember' }, { id: 'skill_js', name: 'JavaScript', bloom_level: 'remember' }]
      if (command === 'list_skill_graph_edges') return []
      if (command === 'get_local_did') return 'did:key:me'
      if (command === 'list_credentials') { state.listCalls++; return state.creds }
      throw new Error(command)
    },
  }),
}))
vi.mock('@/composables/useCredentials', () => ({ useCredentials: () => ({ listDerivedStates: async () => [] }) }))
vi.mock('@/composables/useAuth', () => ({ useAuth: () => ({ vaultUnlocked: ref(true) }) }))
vi.mock('vue-router', () => ({
  useRouter: () => ({
    push: vi.fn(),
    afterEach: (hook: () => void) => { state.navigate.push(hook); return () => {} },
  }),
}))
// The canvas library is irrelevant here; any call on it returns itself.
vi.mock('@/utils/forceGraph', () => {
  const chain: unknown = new Proxy(function () {}, { get: () => () => chain, apply: () => chain })
  return { createForceGraph: async () => chain }
})

import SidebarSkillGraph from './SidebarSkillGraph.vue'
import { announceCredentialsChanged } from '@/composables/credentialEvents'

const credentialFor = (skillId: string) => ({ credentialSubject: { id: 'did:key:me', skillId, level: 0, score: 1 } })
const t = (key: string, params?: Record<string, unknown>) => `${key}${params ? JSON.stringify(params) : ''}`

beforeEach(() => { vi.useFakeTimers(); state.creds = []; state.listCalls = 0; state.navigate = [] })
afterEach(() => { vi.useRealTimers() })

async function settle() {
  await flushPromises(); await vi.advanceTimersByTimeAsync(600); await flushPromises()
}

describe('SidebarSkillGraph earned count', () => {
  it('recounts when a credential is issued, and on navigation after the throttle', async () => {
    const wrapper = mount(SidebarSkillGraph, {
      attachTo: document.body,
      global: { mocks: { $t: t }, stubs: { SkillGraphModal: true } },
    })
    await settle()
    expect(wrapper.text()).toContain('skills.graph.legendEarned{"count":0}')

    // An assessment issues a credential: the panel announces it and the count moves.
    state.creds = [credentialFor('skill_big_o')]
    announceCredentialsChanged()
    await settle()
    expect(wrapper.text()).toContain('skills.graph.legendEarned{"count":1}')
    expect(wrapper.text()).toContain('skills.graph.skillsProgress{"earned":1,"total":2}')

    // A credential arriving some other way shows up on the next navigation...
    state.creds = [credentialFor('skill_big_o'), credentialFor('skill_js')]
    await vi.advanceTimersByTimeAsync(2000)
    state.navigate.forEach((hook) => hook())
    await settle()
    expect(wrapper.text()).toContain('skills.graph.legendEarned{"count":2}')

    // ...but rapid navigation does not re-read on every hop.
    const before = state.listCalls
    state.navigate.forEach((hook) => hook())
    state.navigate.forEach((hook) => hook())
    await flushPromises()
    expect(state.listCalls).toBe(before)
    wrapper.unmount()
  })
})
