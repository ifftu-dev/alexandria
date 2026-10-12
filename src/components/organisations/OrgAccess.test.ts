import { flushPromises, mount } from '@vue/test-utils'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import OrgAccess from './OrgAccess.vue'
import type { GrantView } from '@/types'

const mocks = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock('@/composables/useLocalApi', () => ({ useLocalApi: () => ({ invoke: mocks.invoke }) }))
vi.mock('vue-i18n', () => ({ useI18n: () => ({ t: (key: string) => key, locale: { value: 'en' } }) }))

const CLOUD = 'http://127.0.0.1:8787'

function grant(over: Partial<GrantView> = {}): GrantView {
  return {
    grantId: 'g-1', orgDomain: 'acme.example', orgName: 'Acme Hiring', purpose: 'Backend engineer shortlist',
    skills: [{ skillId: 'skill_big_o', name: 'Big-O Analysis', hasEvidence: true, bloomLevel: 3, level: 4 }],
    sync: false, grantedAt: 1_900_000_000, syncedAt: 1_900_000_000, syncError: null, revokedAt: null,
    lastViewedAt: '2026-10-10T09:00:00Z', viewCount: 3, ...over,
  }
}

function arm(grants: GrantView[]) {
  mocks.invoke.mockImplementation(async (command: string, args?: Record<string, unknown>) => {
    switch (command) {
      case 'org_grants': return { grants, problems: [] }
      case 'fetch_visibility': return { items: [{ directory: 'Alexandria Cloud', organisationId: 'org-9', institution: 'City College', visibleToAdministrators: false }], problems: [] }
      case 'fetch_access_log': return { items: [], problems: [] }
      // The follow-up commands answer from the local record: no view counts.
      case 'org_grant_set_sync': return { ...grant(), sync: args!.sync as boolean, lastViewedAt: null, viewCount: null }
      case 'org_grant_revoke': return { ...grant(), revokedAt: 1_900_100_000, lastViewedAt: null, viewCount: null }
      case 'org_sync_now': return { sent: 1, unchanged: 2, failed: [] }
      case 'set_visibility': return null
      default: throw new Error(command)
    }
  })
}

const mountAccess = () =>
  mount(OrgAccess, {
    props: { cloudUrl: CLOUD },
    global: { mocks: { $t: (key: string) => key }, stubs: { teleport: true } },
  })

const button = (wrapper: ReturnType<typeof mount>, text: string) =>
  wrapper.findAll('button').find((b) => b.text() === text)!

beforeEach(() => { mocks.invoke.mockReset() })

describe('OrgAccess', () => {
  it('shows what each organisation can see, when it looked, and the sync state', async () => {
    arm([grant(), grant({ grantId: 'g-2', orgName: 'Beta Labs', syncError: 'offline', viewCount: 0, lastViewedAt: null })])
    const wrapper = mountAccess(); await flushPromises()
    const cards = wrapper.findAll('[data-testid=grant]')
    expect(cards).toHaveLength(2)
    expect(cards[0]!.text()).toContain('Big-O Analysis')
    expect(cards[0]!.text()).toContain('organisations.access.viewed')
    expect(cards[0]!.text()).toContain('organisations.access.updated')
    expect(cards[1]!.text()).toContain('organisations.access.syncFailed')
    expect(cards[1]!.text()).toContain('organisations.access.notViewed')
  })

  it('turns keeping up to date on through org_grant_set_sync and shows what came back', async () => {
    arm([grant()])
    const wrapper = mountAccess(); await flushPromises()
    const toggle = wrapper.find('[data-testid=grant] [role=switch]')
    expect(toggle.attributes('aria-checked')).toBe('false')
    await toggle.trigger('click'); await flushPromises()
    expect(mocks.invoke).toHaveBeenCalledWith('org_grant_set_sync', { grantId: 'g-1', sync: true })
    expect(wrapper.find('[data-testid=grant] [role=switch]').attributes('aria-checked')).toBe('true')
    // Cloud's view count survives an answer that did not carry one.
    expect(wrapper.find('[data-testid=grant]').text()).toContain('organisations.access.viewed')
  })

  it('stops sharing only after confirming, then lists the grant as no longer shared', async () => {
    arm([grant()])
    const wrapper = mountAccess(); await flushPromises()
    await button(wrapper, 'organisations.access.stop').trigger('click')
    expect(mocks.invoke).not.toHaveBeenCalledWith('org_grant_revoke', expect.anything())
    expect(wrapper.text()).toContain('organisations.access.stopMessage')

    const confirm = wrapper.find('[role=dialog]').findAll('button').find((b) => b.text() === 'organisations.access.stop')!
    await confirm.trigger('click'); await flushPromises()
    expect(mocks.invoke).toHaveBeenCalledWith('org_grant_revoke', { grantId: 'g-1' })
    expect(wrapper.find('[data-testid=grant]').exists()).toBe(false)
    expect(wrapper.find('[data-testid=revoked]').text()).toContain('Acme Hiring')
    expect(wrapper.text()).toContain('organisations.access.empty')
  })

  it('cancelling the confirmation revokes nothing', async () => {
    arm([grant()])
    const wrapper = mountAccess(); await flushPromises()
    await button(wrapper, 'organisations.access.stop').trigger('click')
    await button(wrapper, 'common.actions.cancel').trigger('click'); await flushPromises()
    expect(mocks.invoke).not.toHaveBeenCalledWith('org_grant_revoke', expect.anything())
    expect(wrapper.findAll('[data-testid=grant]')).toHaveLength(1)
  })

  it('collapses revoked grants into a separate list', async () => {
    arm([grant(), grant({ grantId: 'g-old', orgName: 'Old Co', revokedAt: 1_890_000_000 })])
    const wrapper = mountAccess(); await flushPromises()
    expect(wrapper.findAll('[data-testid=grant]')).toHaveLength(1)
    expect(wrapper.find('[data-testid=revoked] summary').text()).toBe('organisations.access.revokedHeading')
    expect(wrapper.find('[data-testid=revoked]').text()).toContain('Old Co')
  })

  it('syncs on request and reports the outcome', async () => {
    arm([grant()])
    const wrapper = mountAccess(); await flushPromises()
    await button(wrapper, 'organisations.access.syncNow').trigger('click'); await flushPromises()
    expect(mocks.invoke).toHaveBeenCalledWith('org_sync_now')
    expect(wrapper.text()).toContain('organisations.access.syncReport')
  })

  it('sends a visibility change to the Cloud it came from', async () => {
    arm([])
    const wrapper = mountAccess(); await flushPromises()
    await button(wrapper, 'organisations.access.visibility.allow').trigger('click'); await flushPromises()
    expect(mocks.invoke).toHaveBeenCalledWith('set_visibility', { directoryUrl: CLOUD, organisationId: 'org-9', visible: true })
  })
})
