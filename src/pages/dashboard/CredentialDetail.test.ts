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
  anchor: vi.fn(),
  retryAnchor: vi.fn(),
  coverageStatement: vi.fn(),
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

async function open(subject: Record<string, unknown> = credential) {
  mocks.get.mockResolvedValue(subject)
  mocks.verify.mockResolvedValue(verification)
  mocks.trust.mockResolvedValue(null)
  if (!mocks.anchor.getMockImplementation()) mocks.anchor.mockResolvedValue(null)
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

const TX = 'ba27e829cb186b6a242cddfeb3f283fd60c8e0c62d7f34f92fccf4f733adad2f'
const anchorState = (over: Record<string, unknown>) => ({
  credential_id: 'urn:alexandria:vc:demo', status: 'pending', tx_hash: null, attempts: 0,
  last_error: null, needs_funds: false, enqueued_at: '2026-11-09T00:00:00Z', confirmed_at: null, ...over,
})

describe('CredentialDetail public record', () => {
  beforeEach(() => {
    Object.values(mocks).forEach(m => m.mockReset())
  })

  it('links a confirmed anchor to its transaction', async () => {
    mocks.anchor.mockResolvedValue(anchorState({ status: 'confirmed', tx_hash: TX, confirmed_at: '2026-11-09T08:10:00Z' }))
    const section = (await open()).find('[data-anchor-section]')
    expect(section.text()).toContain('credentials.anchor.status.confirmed')
    expect(section.text()).toContain('credentials.anchor.body.confirmed')
    expect(section.find('a').attributes('href')).toBe(`https://preprod.cardanoscan.io/transaction/${TX}`)
    expect(section.findAll('button')).toHaveLength(0)
  })

  it('suggests adding tADA only when a balance would fix the error', async () => {
    mocks.anchor.mockResolvedValue(anchorState({
      status: 'pending', last_error: 'needs funds: no sponsor available', needs_funds: true,
    }))
    const section = (await open()).find('[data-anchor-section]')
    expect(section.text()).toContain('credentials.anchor.body.retrying')
    expect(section.text()).toContain('credentials.anchor.body.failedHint')
  })

  it('offers a retry for a failed anchor and shows the new state', async () => {
    mocks.anchor.mockResolvedValue(anchorState({ status: 'failed', attempts: 5, last_error: 'no UTxOs at funding address' }))
    mocks.retryAnchor.mockResolvedValue(anchorState({ status: 'pending' }))
    const wrapper = await open()
    const section = wrapper.find('[data-anchor-section]')
    expect(section.text()).not.toContain('credentials.anchor.body.failedHint')
    expect(section.text()).toContain('no UTxOs at funding address')
    await section.find('button').trigger('click')
    await flushPromises()
    expect(mocks.retryAnchor).toHaveBeenCalledWith('urn:alexandria:vc:demo')
    expect(wrapper.find('[data-anchor-section]').text()).toContain('credentials.anchor.status.pending')
    expect(wrapper.find('[data-anchor-section]').findAll('button')).toHaveLength(0)
  })

  it('says a credential issued elsewhere has no record on this device', async () => {
    mocks.anchor.mockResolvedValue(null)
    const section = (await open()).find('[data-anchor-section]')
    expect(section.text()).toContain('credentials.anchor.body.none')
    expect(section.find('a').exists()).toBe(false)
  })
})

const integrity = {
  sessionId: 'isess-1', status: 'completed', integrityScore: 0.9, criticalCount: 0, warningCount: 0,
  assuranceLevel: 'local', generatedAt: '2026-10-09T00:00:00Z',
  trustProfile: 'core', verificationRung: 'probed', coverageStatementDigest: 'blake3:abc',
  signalsRun: ['core'], signalsDegraded: [], signalsNotRun: ['device_attestation', 'camera'],
  probe: { outcome: 'satisfied', policies: ['reseed'], satisfied: 2, failed: 0, skipped: 0 },
}
const statement = {
  format: 1, version: '1', profile: 'core', rung: 'probed',
  checked: ['Typing, pointer, focus and clipboard behaviour in the assessment window.'],
  not_checked: ['A second person, in the room or helping remotely, or a second device out of view: monitoring cannot see either.'],
  platform_ceilings: ['Overlay, process and display checks see only what each operating system reports.'],
  measurement: 'No signal has a certified error rate yet.',
  certification: null, certified_signals: {},
}

describe('CredentialDetail integrity', () => {
  beforeEach(() => {
    Object.values(mocks).forEach(m => m.mockReset())
  })

  it('names the profile, rung, signals and follow-up questions of the session', async () => {
    const section = (await open({ ...credential, integrity })).get('[data-integrity-section]')
    expect(section.text()).toContain('sentinel.trust.profile.core')
    expect(section.text()).toContain('sentinel.trust.rung.probed')
    expect(section.text()).toContain('sentinel.trust.signal.camera')
    expect(section.text()).toContain('credentials.detail.probeValue')
    expect(section.find('[data-camera-coverage]').exists()).toBe(false)
  })

  it('states the camera coverage beside the camera signal when the credential carries it', async () => {
    const section = (await open({
      ...credential,
      integrity: {
        ...integrity,
        trustProfile: 'camera',
        signalsRun: ['core', 'camera'],
        signalsDegraded: ['camera'],
        signalsNotRun: ['device_attestation'],
        cameraCoverage: 11 / 12,
      },
    })).get('[data-integrity-section]')
    const row = section.get('[data-camera-coverage]')
    expect(row.text()).toContain('credentials.detail.cameraCoverage')
    expect(row.find('.text-warning').exists()).toBe(true)
  })

  it('states where the answers were held, reading a credential that does not say as device', async () => {
    const unstated = (await open({ ...credential, integrity })).get('[data-integrity-section]')
    expect(unstated.get('[data-answer-custody]').text()).toContain('credentials.detail.custody.device')
    const issuer = (await open({ ...credential, integrity: { ...integrity, answerCustody: 'issuer' } })).get('[data-integrity-section]')
    expect(issuer.get('[data-answer-custody]').text()).toContain('credentials.detail.custody.issuer')
  })
})

// What the provenance section states comes from the trust command alone: the
// page renders the view, it never classifies the credential itself.
const trustView = (over: Record<string, unknown> = {}) => ({
  trust: { state: 'verified_self_claim', endorsement: { outcome: 'not_supplied' }, attestation: { outcome: 'not_supplied' } },
  provenance: { kind: 'self_reported', attestation: { outcome: 'not_supplied' } },
  issuer: null,
  organisation: null,
  suspension: { state: 'not_suspended' },
  coverage_statement: null,
  ...over,
})

async function openWithTrust(view: Record<string, unknown>, subject: Record<string, unknown> = credential) {
  mocks.get.mockResolvedValue(subject)
  mocks.verify.mockResolvedValue(verification)
  mocks.trust.mockResolvedValue(view)
  mocks.anchor.mockResolvedValue(null)
  const wrapper = mount(CredentialDetail, { global: { stubs, mocks: { $t: (k: string) => k } } })
  await flushPromises()
  return wrapper.get('[data-trust-section]')
}

describe('CredentialDetail provenance', () => {
  beforeEach(() => {
    Object.values(mocks).forEach(m => m.mockReset())
  })

  it('says a credential without bound evidence is self-reported, from the holder\'s own device', async () => {
    const section = await openWithTrust(trustView())
    expect(section.get('[data-provenance]').text()).toBe('credentials.detail.trustProvenance.selfReported')
    expect(section.text()).toContain('credentials.detail.trustProvenance.selfReportedBody')
    expect(section.text()).not.toContain('credentials.detail.trustProvenance.deviceAttested')
  })

  it('says why evidence that covered only the session still reads as self-reported', async () => {
    const section = await openWithTrust(trustView({
      provenance: { kind: 'self_reported', attestation: { outcome: 'verified_session_bound', platform: 'ios' } },
    }))
    expect(section.get('[data-provenance]').text()).toBe('credentials.detail.trustProvenance.selfReported')
    expect(section.text()).toContain('credentials.detail.trustProvenance.sessionBound')
  })

  it('says a paired phone was present, as a self-reported credential, never device-attested', async () => {
    const section = await openWithTrust(trustView({
      provenance: {
        kind: 'self_reported',
        attestation: { outcome: 'verified_credential_bound', platform: 'ios', companion: true },
      },
    }))
    expect(section.get('[data-provenance]').text()).toBe('credentials.detail.trustProvenance.selfReported')
    expect(section.text()).toContain('credentials.detail.trustProvenance.companion')
    expect(section.text()).not.toContain('credentials.detail.trustProvenance.deviceAttested')
  })

  it('says what device attestation proves and what it does not', async () => {
    const section = await openWithTrust(trustView({
      provenance: { kind: 'device_attested', platform: 'android' },
    }))
    expect(section.get('[data-provenance]').text()).toBe('credentials.detail.trustProvenance.deviceAttested')
    expect(section.text()).toContain('credentials.detail.trustProvenance.deviceAttestedBody')
    expect(section.text()).toContain('credentials.detail.trustProvenance.deviceAttestedLimits')
  })

  it('names the organisation and domain that list the signing key', async () => {
    const section = await openWithTrust(trustView({
      trust: { state: 'verified_issuer_signed', issuer: 'did:key:z6MkOrg', attestation: { outcome: 'not_supplied' }, organisation: { outcome: 'verified', name: 'Org Ltd', domain: 'org.example' } },
      provenance: { kind: 'organisation', name: 'Org Ltd', domain: 'org.example' },
      issuer: 'did:key:z6MkOrg',
      organisation: { outcome: 'verified', name: 'Org Ltd', domain: 'org.example' },
    }))
    expect(section.get('[data-provenance]').text()).toBe(
      'credentials.detail.trustProvenance.organisation {"name":"Org Ltd","domain":"org.example"}',
    )
  })

  it('says when the domain no longer lists the key that signed it', async () => {
    const section = await openWithTrust(trustView({
      trust: { state: 'verified_issuer_signed', issuer: 'did:key:z6MkOrg', attestation: { outcome: 'not_supplied' }, organisation: { outcome: 'key_not_listed' } },
      issuer: 'did:key:z6MkOrg',
      organisation: { outcome: 'key_not_listed' },
    }))
    expect(section.get('[data-provenance]').text()).toBe('credentials.detail.trustProvenance.selfReported')
    expect(section.text()).toContain('credentials.detail.trustProvenance.signedBy {"issuer":"did:key:z6MkOrg"}')
    expect(section.text()).toContain('credentials.detail.trustProvenance.keyNotListed')
  })

  it('states the provenance of a suspended credential and that it is suspended', async () => {
    const section = await openWithTrust(trustView({
      trust: { state: 'invalid', reasons: ['suspended'] },
      suspension: { state: 'suspended' },
    }))
    expect(section.get('[data-provenance]').text()).toBe('credentials.detail.trustProvenance.selfReported')
    expect(section.get('[data-suspension]').text()).toBe('credentials.detail.trustSuspension.suspended')
  })

  it('says suspension is unknown while the suspension list is missing', async () => {
    const section = await openWithTrust(trustView({
      trust: { state: 'pending', reasons: ['suspension_list_missing'] },
      provenance: null,
      suspension: { state: 'unknown', reason: 'suspension_list_missing' },
    }))
    expect(section.find('[data-provenance]').exists()).toBe(false)
    expect(section.text()).toContain('credentials.detail.trustState.pending')
    expect(section.get('[data-suspension]').text()).toBe('credentials.detail.trustSuspension.unknown')
  })

  it('shows the coverage statement the trust command resolved', async () => {
    const section = await openWithTrust(trustView({ coverage_statement: statement }), { ...credential, integrity })
    expect(mocks.coverageStatement).not.toHaveBeenCalled()
    expect(section.get('[data-testid="coverage-statement"]').text()).toContain('a second device out of view')
  })

  it('says below the live rung, in the statement\'s own words, that a second device or person was not ruled out', async () => {
    const line = 'Whether a second device or person helped: not ruled out. Only a live segment, where a person asks unscripted questions, rules it out.'
    const below = await openWithTrust(
      trustView({ coverage_statement: { ...statement, version: '3', not_checked: [...statement.not_checked, line] } }),
      { ...credential, integrity },
    )
    expect(below.get('[data-second-device]').text()).toBe(line)

    // A `live` statement has no such line, and neither has an older version.
    const live = await openWithTrust(
      trustView({ coverage_statement: { ...statement, version: '3', rung: 'live' } }),
      { ...credential, integrity: { ...integrity, verificationRung: 'live' } },
    )
    expect(live.find('[data-second-device]').exists()).toBe(false)
  })

  it('says when the named coverage statement could not be resolved', async () => {
    const section = await openWithTrust(trustView(), { ...credential, integrity })
    const coverage = section.get('[data-testid="coverage-statement"]')
    expect(coverage.text()).toContain('sentinel.trust.coverage.unresolved')
    expect(coverage.text()).toContain('blake3:abc')
  })
})

describe('CredentialDetail suspension check', () => {
  beforeEach(() => {
    Object.values(mocks).forEach(m => m.mockReset())
  })

  it('shows suspension as pending, not "no", while the suspension list is missing', async () => {
    mocks.get.mockResolvedValue(credential)
    mocks.verify.mockResolvedValue({ ...verification, acceptanceDecision: 'pending', pendingReasons: ['suspension_list_missing'] })
    mocks.trust.mockResolvedValue(null)
    mocks.anchor.mockResolvedValue(null)
    const wrapper = mount(CredentialDetail, { global: { stubs, mocks: { $t: (k: string) => k } } })
    await flushPromises()
    const row = wrapper.get('[data-suspended-check]')
    expect(row.text()).toContain('credentials.value.pending')
  })
})
