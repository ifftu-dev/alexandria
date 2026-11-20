import { mount } from '@vue/test-utils'
import { describe, expect, it, vi } from 'vitest'
import RecordConsent from './RecordConsent.vue'
import type { RecordConsentDecision } from '@/types'

vi.mock('vue-i18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }))

const COURSES = [
  { id: 'c-safety', title: 'Warehouse safety' },
  { id: 'c-forklift', title: 'Forklift basics' },
]
const PAIRWISE = 'z6MkpairwiseForThisStoreOnly'
const REAL = 'z6MkTheLearnersOwnDid'

function mountConsent(props: Record<string, unknown> = {}) {
  return mount(RecordConsent, {
    props: {
      recipient: 'https://lrs.example.org/xapi/',
      courses: COURSES,
      pairwiseActor: PAIRWISE,
      realDidActor: REAL,
      ...props,
    },
    global: { mocks: { $t: (key: string) => key } },
  })
}

const approve = (wrapper: ReturnType<typeof mountConsent>) => wrapper.get('[data-testid=consent-approve]')

function decision(wrapper: ReturnType<typeof mountConsent>): RecordConsentDecision {
  const emitted = wrapper.emitted('approve')
  expect(emitted).toHaveLength(1)
  return emitted![0]![0] as RecordConsentDecision
}

describe('RecordConsent', () => {
  it('shows the scope, the identifier records will carry and what removal can and cannot undo', () => {
    const wrapper = mountConsent({ initialCourseIds: ['c-safety'] })
    const text = wrapper.text()
    expect(text).toContain('Warehouse safety')
    expect(text).toContain('Forklift basics')
    expect(text).toContain('organisations.records.consent.completionAlways')
    expect(text).toContain('organisations.records.consent.interactionsNote')
    expect(wrapper.get('[data-testid=consent-actor]').text()).toBe(PAIRWISE)
    expect(text).toContain('organisations.records.consent.undoStops')
    expect(text).toContain('organisations.records.consent.undoCannot')
    expect(text).toContain('organisations.records.consent.nothingYet')
  })

  it('shares completion-level records by default; individual responses are a separate choice', async () => {
    const wrapper = mountConsent({ initialCourseIds: ['c-safety'] })
    const responses = wrapper.get<HTMLInputElement>('[data-testid=consent-interactions]')
    expect(responses.element.checked).toBe(false)
    await approve(wrapper).trigger('click')
    expect(decision(wrapper)).toEqual({
      course_ids: ['c-safety'],
      include_interactions: false,
      identifier: 'pairwise',
      confirm_private: false,
    })
  })

  it('approves a narrower scope than was offered', async () => {
    const wrapper = mountConsent({ initialCourseIds: ['c-safety', 'c-forklift'] })
    await wrapper.get('[data-testid=consent-course-c-forklift]').setValue(false)
    await wrapper.get('[data-testid=consent-interactions]').setValue(true)
    await approve(wrapper).trigger('click')
    const chosen = decision(wrapper)
    expect(chosen.course_ids).toEqual(['c-safety'])
    expect(chosen.include_interactions).toBe(true)
  })

  it('shows the learner their own identifier before they choose to use it', async () => {
    const wrapper = mountConsent({ initialCourseIds: ['c-safety'] })
    await wrapper.get('[data-testid=consent-identifier-real_did]').setValue(true)
    expect(wrapper.get('[data-testid=consent-actor]').text()).toBe(REAL)
    expect(wrapper.text()).toContain('organisations.records.consent.realDidNote')
    await approve(wrapper).trigger('click')
    expect(decision(wrapper).identifier).toBe('real_did')
  })

  it('cannot be approved without a course, and a private address needs its own confirmation', async () => {
    const wrapper = mountConsent({ privateAddress: true })
    expect(approve(wrapper).attributes('disabled')).toBeDefined()
    await wrapper.get('[data-testid=consent-course-c-safety]').setValue(true)
    expect(approve(wrapper).attributes('disabled')).toBeDefined()
    expect(wrapper.text()).toContain('organisations.records.consent.privateAddress')
    await wrapper.get('[data-testid=consent-confirm-private]').setValue(true)
    await approve(wrapper).trigger('click')
    expect(decision(wrapper).confirm_private).toBe(true)
  })

  it('hides individual responses when the recipient did not ask for them', () => {
    const wrapper = mountConsent({ interactionsOffered: false, initialCourseIds: ['c-safety'] })
    expect(wrapper.find('[data-testid=consent-interactions]').exists()).toBe(false)
  })

  it('cancelling sends nothing', async () => {
    const wrapper = mountConsent({ initialCourseIds: ['c-safety'] })
    await wrapper.get('[data-testid=consent-cancel]').trigger('click')
    expect(wrapper.emitted('cancel')).toHaveLength(1)
    expect(wrapper.emitted('approve')).toBeUndefined()
  })
})
