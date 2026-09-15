import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import ImportGenesis from './ImportGenesis.vue'
import type {
  GovernanceGenesisLocator,
  PinGenesisResponse,
  RetrievedGenesisPreview,
} from '@/types'

const mocks = vi.hoisted(() => ({
  invoke: vi.fn<(command: string, args?: Record<string, unknown>) => Promise<unknown>>(),
}))

const daoId = 'ab'.repeat(32)
const contentHash = 'cd'.repeat(32)
const locatorUri = `alexandria://governance/genesis/${daoId}?content=${contentHash}&source=iroh%3A%2F%2F${contentHash}&source=https%3A%2F%2Fmirror.example%2Fgenesis.json%23blake3%3D${contentHash}`
const locator: GovernanceGenesisLocator = {
  version: 1,
  dao_id: daoId,
  content_hash: contentHash,
  locations: [
    `https://mirror.example/genesis.json#blake3=${contentHash}`,
    `iroh://${contentHash}`,
  ],
}
const retrieved: RetrievedGenesisPreview = {
  locator,
  resolved_from: contentHash,
  genesis_json: '{"canonical":true}',
  preview: {
    dao_id: daoId,
    genesis_hash: daoId,
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
      member_id: `member-${index}`,
      identity_public_key_hex: `${index}`.repeat(64),
      consensus_public_key_hex: `${index + 1}`.repeat(64),
      governance_public_key_hex: `${index + 2}`.repeat(64),
    })),
  },
}

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
  default: { props: ['value'], template: '<div data-qr="true" />' },
}))

function render() {
  return mount(ImportGenesis, {
    global: {
      mocks: { $t: (key: string) => key },
      stubs: { RouterLink: { template: '<a><slot /></a>' } },
    },
  })
}

beforeEach(() => {
  vi.clearAllMocks()
  mocks.invoke.mockImplementation(async command => {
    if (command === 'governance_preview_genesis_locator') return locator
    if (command === 'governance_retrieve_genesis') return retrieved
    if (command === 'governance_pin_genesis') {
      return { preview: retrieved.preview, newly_pinned: true } satisfies PinGenesisResponse
    }
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

    const retrieve = wrapper.findAll('button').find(button =>
      button.text().includes('governanceGenesisImport.retrieve'),
    )
    expect(retrieve).toBeDefined()
    await retrieve!.trigger('click')
    await flushPromises()
    expect(mocks.invoke).toHaveBeenCalledWith('governance_retrieve_genesis', { locatorUri })
    expect(wrapper.text()).toContain('Computing DAO')

    const pin = wrapper.findAll('button').find(button =>
      button.text().includes('governanceGenesisImport.pin'),
    )
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
  })
})
