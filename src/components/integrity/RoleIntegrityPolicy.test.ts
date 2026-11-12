import { flushPromises, mount } from '@vue/test-utils'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import RoleIntegrityPolicy from './RoleIntegrityPolicy.vue'
import { type RoleIntegrityChoice, integrityChoiceReady, integrityPolicyFields, noIntegrityMinimums } from '@/utils/sentinel/rolePolicy'
import type { CoverageStatement, TrustProfile, VerificationRung } from '@/types'

// The sponsor's three integrity minimums for a role, each with plain
// language beside it and the coverage statement a credential under that
// policy would name.
const mocks = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock('@/composables/useLocalApi', () => ({ useLocalApi: () => ({ invoke: mocks.invoke }) }))

function statement(profile: TrustProfile, rung: VerificationRung): CoverageStatement {
  return {
    format: 1, version: '1', profile, rung,
    checked: [`checked under ${profile}/${rung}`],
    not_checked: ['A second person or a second device: monitoring cannot see either.'],
    platform_ceilings: [], measurement: 'advisory', certification: null, certified_signals: {},
  }
}

const empty: RoleIntegrityChoice = noIntegrityMinimums()

function render(modelValue: RoleIntegrityChoice) {
  return mount(RoleIntegrityPolicy, {
    props: { modelValue },
    global: { mocks: { $t: (key: string) => key } },
  })
}

beforeEach(() => {
  mocks.invoke.mockReset().mockImplementation(async (command: string) => {
    if (command === 'studio_list_connections') {
      return [{ value: { id: 'conn-1', name: 'Team model', enabled: true, capability: 'text' } }]
    }
    return [statement('core', 'none'), statement('camera', 'none'), statement('camera', 'probed')]
  })
})

describe('RoleIntegrityPolicy', () => {
  it('shows the coverage of the chosen minimums, core and no rung by default', async () => {
    const wrapper = render(empty)
    await flushPromises()
    expect(mocks.invoke).toHaveBeenCalledWith('integrity_coverage_statements')
    expect(wrapper.get('[data-testid="coverage-statement"]').text()).toContain('checked under core/none')

    await wrapper.setProps({ modelValue: { ...empty, min_trust_profile: 'camera', min_verification_rung: 'probed' } })
    expect(wrapper.get('[data-testid="coverage-statement"]').text()).toContain('checked under camera/probed')
    expect(wrapper.text()).toContain('sentinel.trust.profileHint.camera')
    expect(wrapper.text()).toContain('sentinel.trust.rungHint.probed')
  })

  it('emits each choice as it changes', async () => {
    const wrapper = render(empty)
    await flushPromises()
    await wrapper.get('[data-testid="policy-min-profile"]').setValue('attested')
    const emitted = () => wrapper.emitted('update:modelValue') ?? []
    expect(emitted()[emitted().length - 1]).toEqual([{ ...empty, min_trust_profile: 'attested' }])
    await wrapper.get('[data-testid="policy-probe"]').setValue('reseed')
    expect(emitted()[emitted().length - 1]).toEqual([{ ...empty, probe_policy: 'reseed' }])
  })

  it('turns the choice into issuance policy fields, leaving out what is unset', () => {
    expect(integrityPolicyFields(empty)).toEqual({})
    expect(integrityPolicyFields({ ...empty, min_trust_profile: 'full', min_verification_rung: 'consistent', probe_policy: 'authored' }))
      .toEqual({ min_trust_profile: 'full', min_verification_rung: 'consistent', probe_policy: 'authored' })
    expect(integrityPolicyFields({ ...empty, probe_policy: 'llm', probe_connection_id: 'conn-1' }))
      .toEqual({ probe_policy: 'llm', probe_connection_id: 'conn-1' })
  })

  it('asks for a model connection only for generated follow-ups', async () => {
    const wrapper = render(empty)
    await flushPromises()
    expect(wrapper.find('[data-testid="generated-probe-connection"]').exists()).toBe(false)
    expect(integrityChoiceReady({ ...empty, probe_policy: 'llm' })).toBe(false)

    await wrapper.setProps({ modelValue: { ...empty, probe_policy: 'llm' } })
    await flushPromises()
    await wrapper.get('[data-testid="generated-probe-connection"] select').setValue('conn-1')
    const emitted = () => wrapper.emitted('update:modelValue') ?? []
    expect(emitted()[emitted().length - 1]).toEqual([{ ...empty, probe_policy: 'llm', probe_connection_id: 'conn-1' }])

    await wrapper.setProps({ modelValue: { ...empty, probe_policy: 'llm', probe_connection_id: 'conn-1' } })
    await wrapper.get('[data-testid="policy-probe"]').setValue('reseed')
    expect(emitted()[emitted().length - 1]).toEqual([{ ...empty, probe_policy: 'reseed' }])
  })
})
