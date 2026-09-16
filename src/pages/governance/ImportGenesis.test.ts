import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import ImportGenesis from './ImportGenesis.vue'
import type {
  PinGenesisResponse,
  RetrievedGenesisPreview,
  ReviewedGenesisLocator,
} from '@/types'

const mocks = vi.hoisted(() => ({
  invoke: vi.fn<(command: string, args?: Record<string, unknown>) => Promise<unknown>>(),
}))

const daoId = 'ab'.repeat(32)
const contentHash = 'cd'.repeat(32)
// What the user typed: an official app link with a non-canonical mirror spelling.
const locatorUri = `https://alexandria.ifftu.dev/governance/genesis/${daoId}?content=${contentHash}&source=iroh%3A%2F%2F${contentHash}&source=https%3A%2F%2FMirror.Example.ORG%3A443%2Fgenesis.json%3F%23blake3%3D${contentHash}`
// What review returned: the only form retrieval and sharing may use.
const canonicalUri = `alexandria://governance/genesis/${daoId}?content=${contentHash}&source=https%3A%2F%2Fmirror.example.org%2Fgenesis.json%23blake3%3D${contentHash}&source=iroh%3A%2F%2F${contentHash}`
const reviewed: ReviewedGenesisLocator = {
  version: 1,
  dao_id: daoId,
  content_hash: contentHash,
  locations: [
    `https://mirror.example.org/genesis.json#blake3=${contentHash}`,
    `iroh://${contentHash}`,
  ],
  canonical_uri: canonicalUri,
}
const { canonical_uri: _canonical, ...locator } = reviewed

function retrievedWith(overrides: Partial<RetrievedGenesisPreview['preview']> = {}): RetrievedGenesisPreview {
  return {
    locator,
    resolved_from: contentHash,
    genesis_json: '{"canonical":true}',
    preview: {
      dao_id: daoId,
      core_hash: daoId,
      envelope_hash: contentHash,
      name: 'Computing DAO',
      scope_type: 'subject',
      scope_id: 'computer-science',
      protocol_version: 1,
      rules_version: '1',
      rules_hash: 'ef'.repeat(32),
      proposal_approval_numerator: 2,
      proposal_approval_denominator: 3,
      minimum_turnout_count: 25,
      committee_size: 7,
      receipt_threshold: 5,
      outcome_threshold: 5,
      qualification_policy_version: '1',
      accepted_issuers: ['did:key:issuer'],
      accepted_assessment_evidence: ['assessment-credential'],
      cometbft_chain_id: 'alexandria-computing-1',
      initial_epoch: 0,
      initial_height: 1,
      activation_time_unix: 1_800_000_000,
      members: Array.from({ length: 7 }, (_, index) => ({
        member_id: `did:key:z6Mkmember${index}`,
        identity_public_key_hex: `${index}`.repeat(64),
        consensus_public_key_hex: `${index + 1}`.repeat(64),
        governance_public_key_hex: `${index + 2}`.repeat(64),
      })),
      ...overrides,
    },
  }
}
const retrieved = retrievedWith()

vi.mock('vue-router', () => ({
  useRoute: () => ({ query: { locator: locatorUri } }),
}))
vi.mock('vue-i18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }))
vi.mock('@/composables/useLocalApi', () => ({ useLocalApi: () => ({ invoke: mocks.invoke }) }))
vi.mock('@/components/ui', async () => ({
  AppButton: (await import('@/components/ui/AppButton.vue')).default,
  AppInput: (await import('@/components/ui/AppInput.vue')).default,
}))
vi.mock('@/components/tutoring/QrCodeDisplay.vue', () => ({
  default: { props: ['value'], template: '<div data-qr="true" :data-value="value" />' },
}))

function render() {
  return mount(ImportGenesis, {
    global: {
      mocks: { $t: (key: string) => key },
      stubs: { RouterLink: { template: '<a><slot /></a>' } },
    },
  })
}

function button(wrapper: ReturnType<typeof render>, key: string) {
  return wrapper.findAll('button').find(candidate => candidate.text().includes(key))
}

function pinResponse(overrides: Partial<PinGenesisResponse> = {}): PinGenesisResponse {
  return { preview: retrieved.preview, newly_pinned: true, stored_envelope_differs: false, ...overrides }
}

beforeEach(() => {
  vi.clearAllMocks()
  mocks.invoke.mockImplementation(async command => {
    if (command === 'governance_preview_genesis_locator') return reviewed
    if (command === 'governance_retrieve_genesis') return retrieved
    if (command === 'governance_pin_genesis') return pinResponse()
    throw new Error(`unexpected command: ${command}`)
  })
})

describe('governance genesis import', () => {
  it('opens a deep link as locator review without fetching or pinning', async () => {
    const wrapper = render()
    await flushPromises()

    expect(mocks.invoke.mock.calls).toEqual([
      ['governance_preview_genesis_locator', { locatorUri }],
    ])
    expect(wrapper.text()).toContain(daoId)
    expect(wrapper.text()).not.toContain('Computing DAO')
  })

  it('requires separate retrieval and an exact typed DAO id before pinning', async () => {
    const wrapper = render()
    await flushPromises()

    const retrieve = button(wrapper, 'governanceGenesisImport.retrieve')
    expect(retrieve).toBeDefined()
    await retrieve!.trigger('click')
    await flushPromises()
    expect(wrapper.text()).toContain('Computing DAO')

    const pin = button(wrapper, 'governanceGenesisImport.pin')
    expect(pin).toBeDefined()
    expect((pin!.element as HTMLButtonElement).disabled).toBe(true)

    await wrapper.get('input').setValue(daoId)
    expect((pin!.element as HTMLButtonElement).disabled).toBe(false)
    await pin!.trigger('click')
    await flushPromises()
    expect(mocks.invoke).toHaveBeenCalledWith('governance_pin_genesis', {
      genesisJson: retrieved.genesis_json,
      expectedDaoId: daoId,
    })
    expect(wrapper.text()).toContain('governanceGenesisImport.pinned')
  })

  it('retrieves and shares only the reviewed canonical locator, not the typed text', async () => {
    const wrapper = render()
    await flushPromises()

    expect(wrapper.get('[data-qr="true"]').attributes('data-value')).toBe(canonicalUri)
    expect(wrapper.get('[data-testid="canonical-locator"]').text()).toBe(canonicalUri)

    await button(wrapper, 'governanceGenesisImport.retrieve')!.trigger('click')
    await flushPromises()
    expect(mocks.invoke).toHaveBeenCalledWith('governance_retrieve_genesis', {
      locatorUri: canonicalUri,
    })
    expect(mocks.invoke).not.toHaveBeenCalledWith('governance_retrieve_genesis', { locatorUri })
  })

  it('resets the review, retrieval and confirmation when the locator text changes', async () => {
    const wrapper = render()
    await flushPromises()
    await button(wrapper, 'governanceGenesisImport.retrieve')!.trigger('click')
    await flushPromises()
    await wrapper.get('input').setValue(daoId)
    expect(wrapper.find('[data-testid="trust-facts"]').exists()).toBe(true)

    await wrapper.get('textarea').setValue(`${locatorUri}&source=https%3A%2F%2Fother.example.net`)
    expect(wrapper.find('[data-testid="locator-facts"]').exists()).toBe(false)
    expect(wrapper.find('[data-testid="trust-facts"]').exists()).toBe(false)
    expect(wrapper.find('[data-qr="true"]').exists()).toBe(false)
    expect(button(wrapper, 'governanceGenesisImport.retrieve')).toBeUndefined()
    expect(button(wrapper, 'governanceGenesisImport.pin')).toBeUndefined()
  })

  it('discards a review that finishes after the locator text changed', async () => {
    let resolveReview: (value: ReviewedGenesisLocator) => void = () => {}
    mocks.invoke.mockImplementation(async command => {
      if (command === 'governance_preview_genesis_locator') {
        return new Promise<ReviewedGenesisLocator>(resolve => {
          resolveReview = resolve
        })
      }
      throw new Error(`unexpected command: ${command}`)
    })
    const wrapper = render()
    await flushPromises()

    await wrapper.get('textarea').setValue('alexandria://governance/genesis/edited')
    resolveReview(reviewed)
    await flushPromises()
    expect(wrapper.find('[data-testid="locator-facts"]').exists()).toBe(false)
  })

  it('refuses to show trust facts when retrieval answers for a different locator', async () => {
    mocks.invoke.mockImplementation(async command => {
      if (command === 'governance_preview_genesis_locator') return reviewed
      if (command === 'governance_retrieve_genesis') {
        return {
          ...retrieved,
          locator: { ...locator, locations: [`iroh://${contentHash}`, 'https://other.example.net/g'] },
        }
      }
      throw new Error(`unexpected command: ${command}`)
    })
    const wrapper = render()
    await flushPromises()
    await button(wrapper, 'governanceGenesisImport.retrieve')!.trigger('click')
    await flushPromises()

    expect(wrapper.text()).toContain('governanceGenesisImport.retrievedMismatch')
    expect(wrapper.find('[data-testid="trust-facts"]').exists()).toBe(false)
  })

  it('renders invisible format characters in names and ids visibly', async () => {
    const spoofed = retrievedWith({
      name: 'Computing DAO ‮lanoiciffo',
      members: retrieved.preview.members.map((member, index) =>
        index === 0 ? { ...member, member_id: `did:key:z6Mk​spoof` } : member,
      ),
    })
    mocks.invoke.mockImplementation(async command => {
      if (command === 'governance_preview_genesis_locator') return reviewed
      if (command === 'governance_retrieve_genesis') return spoofed
      throw new Error(`unexpected command: ${command}`)
    })
    const wrapper = render()
    await flushPromises()
    await button(wrapper, 'governanceGenesisImport.retrieve')!.trigger('click')
    await flushPromises()

    const name = wrapper.get('[data-testid="genesis-name"]')
    expect(name.find('bdi').exists()).toBe(true)
    expect(name.text()).toBe('Computing DAO [U+202E]lanoiciffo')
    expect(wrapper.text()).toContain('did:key:z6Mk[U+200B]spoof')
    expect(wrapper.html()).not.toContain('‮')
    expect(wrapper.html()).not.toContain('​')
  })

  it('reports an equivalent, differently signed pin without claiming a new anchor', async () => {
    mocks.invoke.mockImplementation(async command => {
      if (command === 'governance_preview_genesis_locator') return reviewed
      if (command === 'governance_retrieve_genesis') return retrieved
      if (command === 'governance_pin_genesis') {
        return pinResponse({ newly_pinned: false, stored_envelope_differs: true })
      }
      throw new Error(`unexpected command: ${command}`)
    })
    const wrapper = render()
    await flushPromises()
    await button(wrapper, 'governanceGenesisImport.retrieve')!.trigger('click')
    await flushPromises()
    await wrapper.get('input').setValue(daoId)
    await button(wrapper, 'governanceGenesisImport.pin')!.trigger('click')
    await flushPromises()

    expect(wrapper.text()).toContain('governanceGenesisImport.equivalentAlreadyPinned')
    expect(wrapper.text()).not.toContain('governanceGenesisImport.pinned')
  })
})
