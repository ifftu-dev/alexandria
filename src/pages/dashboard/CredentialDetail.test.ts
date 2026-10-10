import { flushPromises, mount } from '@vue/test-utils'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { defineComponent, ref } from 'vue'
import CredentialDetail from './CredentialDetail.vue'

// The revoke screen: pressing Revoke calls the revoke command, then the
// publish command, and tells the learner where the status list landed (or
// why it did not) — the step a live demo depends on seeing.
const mocks = vi.hoisted(() => ({
  get: vi.fn(),
  verify: vi.fn(),
  trust: vi.fn(),
  revoke: vi.fn(),
  publishStatusLists: vi.fn(),
}))
vi.mock('@/composables/useCredentials', () => ({
  useCredentials: () => ({ ...mocks, error: ref<string | null>(null) }),
}))
vi.mock('vue-router', () => ({
  useRoute: () => ({ params: { id: 'urn:alexandria:vc:demo' } }),
  useRouter: () => ({ push: vi.fn(), back: vi.fn() }),
}))
vi.mock('vue-i18n', () => ({
  useI18n: () => ({ t: (key: string, args?: Record<string, unknown>) => (args ? `${key} ${JSON.stringify(args)}` : key) }),
}))

const credential = {
  '@context': ['https://www.w3.org/ns/credentials/v2'],
  id: 'urn:alexandria:vc:demo',
  type: ['VerifiableCredential', 'AssessmentCredential'],
  issuer: 'did:key:z6MkIssuer',
  validFrom: '2026-10-09T00:00:00Z',
  credentialSubject: { id: 'did:key:z6MkIssuer', skillId: 'skill_javascript', level: 1, score: 1 },
  credentialStatus: {
    id: 'http://127.0.0.1:8787/status-lists/did:key:z6MkIssuer/1#0',
    type: 'BitstringStatusListEntry',
    statusPurpose: 'revocation',
    statusListIndex: '0',
    statusListCredential: 'http://127.0.0.1:8787/status-lists/did:key:z6MkIssuer/1',
  },
  proof: { type: 'DataIntegrityProof', cryptosuite: 'eddsa-jcs-2022', created: '2026-10-09T00:00:00Z', verificationMethod: 'did:key:z6MkIssuer#z6MkIssuer', proofPurpose: 'assertionMethod', proofValue: 'z1' },
}
const verification = {
  credentialId: 'urn:alexandria:vc:demo', validSignature: true, issuerResolved: true, revoked: false, statusValid: true,
  expired: false, subjectBound: true, suspended: false, superseded: false, integrityAnchored: false,
  acceptanceDecision: 'accept', pendingReasons: [], verificationTime: '2026-10-09T00:00:00Z',
}

const stubs = {
  AppButton: defineComponent({ props: ['variant', 'loading'], template: '<button :data-variant="variant" :disabled="loading"><slot /></button>' }),
  AppBadge: defineComponent({ template: '<span><slot /></span>' }),
  AppAlert: defineComponent({ props: ['variant', 'message'], template: '<div role="alert" :data-variant="variant">{{ message }}</div>' }),
  RouterLink: defineComponent({ template: '<a><slot /></a>' }),
}

async function open() {
  mocks.get.mockResolvedValue(credential)
  mocks.verify.mockResolvedValue(verification)
  mocks.trust.mockResolvedValue(null)
  const wrapper = mount(CredentialDetail, { global: { stubs, mocks: { $t: (k: string) => k } } })
  await flushPromises()
  return wrapper
}

async function pressRevoke(wrapper: Awaited<ReturnType<typeof open>>) {
  const open = wrapper.findAll('button').find(b => b.text() === 'credentials.detail.revoke')
  await open!.trigger('click')
  await flushPromises()
  const dangers = wrapper.findAll('button[data-variant="danger"]')
  const confirm = dangers[dangers.length - 1]
  await confirm!.trigger('click')
  await flushPromises()
}

describe('CredentialDetail revoke', () => {
  beforeEach(() => {
    Object.values(mocks).forEach(m => m.mockReset())
    mocks.revoke.mockResolvedValue(undefined)
  })

  it('revokes, publishes the status list and says which host now serves it', async () => {
    mocks.publishStatusLists.mockResolvedValue({ published: ['http://127.0.0.1:8787/status-lists/did:key:z6MkIssuer/1'], errors: [] })
    const wrapper = await open()
    await pressRevoke(wrapper)
    expect(mocks.revoke).toHaveBeenCalledWith('urn:alexandria:vc:demo', 'credentials.detail.revokeDefaultReason')
    expect(mocks.publishStatusLists).toHaveBeenCalledTimes(1)
    const alert = wrapper.find('[role="alert"]')
    expect(alert.attributes('data-variant')).toBe('success')
    expect(alert.text()).toContain('credentials.detail.statusPublished')
    expect(alert.text()).toContain('http://127.0.0.1:8787')
    // Re-verified after revocation so the badge reflects the new state.
    expect(mocks.verify).toHaveBeenCalledTimes(2)
  })

  it('keeps the revocation and warns when the host could not be reached', async () => {
    mocks.publishStatusLists.mockResolvedValue({ published: [], errors: ['http://127.0.0.1:8787/status-lists/did:key:z6MkIssuer/1: connection refused'] })
    const wrapper = await open()
    await pressRevoke(wrapper)
    expect(mocks.revoke).toHaveBeenCalledTimes(1)
    const alert = wrapper.find('[role="alert"]')
    expect(alert.attributes('data-variant')).toBe('warning')
    expect(alert.text()).toContain('credentials.detail.statusPublishFailed')
    expect(alert.text()).toContain('connection refused')
  })
})
