import { flushPromises, mount } from '@vue/test-utils'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import DeveloperSeedsPanel from './DeveloperSeedsPanel.vue'
import type { SeedResource } from '@/types'

const mocks = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock('@/composables/useLocalApi', () => ({ useLocalApi: () => ({ invoke: mocks.invoke }) }))
vi.mock('@/composables/useProfiles', () => ({ useProfiles: () => ({ profiles: { value: [{ id: 'one', display_name: 'Test learner' }] }, activeProfileId: { value: 'one' } }) }))
const resources: SeedResource[] = [
  { id: 'video:lab', title: 'Video lab', category: 'Courses', description: 'Local course', dependencies: ['media:clip'], installed: false },
  { id: 'media:clip', title: 'Clip', category: 'Media', description: 'Local video', dependencies: [], installed: false },
  { id: 'bank:js', title: 'JS questions', category: 'Assessments', description: 'Questions', dependencies: [], installed: true },
]
const render = () => mount(DeveloperSeedsPanel, { global: { stubs: { RouterLink: true } } })
function button(wrapper: ReturnType<typeof render>, label: string) {
  const b = wrapper.findAll('button').find(b => b.text() === label)
  if (!b) throw new Error(`Missing button ${label}`)
  return b
}
beforeEach(() => {
  mocks.invoke.mockReset().mockImplementation(async (command: string) => {
    if (command === 'dev_seed_catalog') return { enabled: true, resources }
    if (command === 'dev_seed_plan') return resources
    if (command === 'dev_seed_reset_plan') return { token: 'reviewed-data', resources: [{ id: 'bank:js', title: 'JS questions', can_reset: true, reason: null, effects: [{ label: 'Assessment attempts and results', count: 3, action: 'remove' }] }] }
    if (command === 'dev_seed_reset_run') return [{ id: 'bank:js', status: 'removed', error: null }]
    if (command === 'dev_seed_run') return resources.map(r => ({ id: r.id, status: r.installed ? 'kept' : 'added', error: null }))
    throw new Error(command)
  })
})
describe('Developer seed picker', () => {
  it('hides the picker when the backend disables it', async () => {
    mocks.invoke.mockResolvedValue({ enabled: false, resources: [] })
    const wrapper = render(); await flushPromises()
    expect(wrapper.find('section').exists()).toBe(false)
  })
  it('retains selections across categories and reviews dependencies before writing', async () => {
    const wrapper = render(); await flushPromises()
    await button(wrapper, 'Courses').trigger('click')
    await wrapper.find('input[type=checkbox]').setValue(true)
    await button(wrapper, 'Assessments').trigger('click')
    await wrapper.find('input[type=checkbox]').setValue(true)
    await button(wrapper, 'Review selection').trigger('click'); await flushPromises()
    expect(mocks.invoke).toHaveBeenCalledWith('dev_seed_plan', { selected: ['video:lab', 'bank:js'] })
    expect(wrapper.text()).toContain('required dependency')
    expect(mocks.invoke.mock.calls.some(c => c[0] === 'dev_seed_run')).toBe(false)
  })
  it('seeds everything in one reviewed run and displays per-resource results', async () => {
    const wrapper = render(); await flushPromises()
    await button(wrapper, 'Seed everything').trigger('click'); await flushPromises()
    expect(mocks.invoke).toHaveBeenCalledWith('dev_seed_plan', { selected: resources.map(r => r.id) })
    await button(wrapper, 'Seed these resources').trigger('click'); await flushPromises()
    expect(mocks.invoke).toHaveBeenCalledWith('dev_seed_run', { selected: resources.map(r => r.id) })
    expect(wrapper.text()).toContain('2 added · 1 kept · 0 failed')
    expect(mocks.invoke.mock.calls.filter(c => c[0] === 'dev_seed_run')).toHaveLength(1)
  })
  it('reviews affected data and requires confirmation before resetting all installed seeds', async () => {
    const wrapper = render(); await flushPromises()
    await button(wrapper, 'Reset all seeds').trigger('click'); await flushPromises()
    expect(mocks.invoke).toHaveBeenCalledWith('dev_seed_reset_plan', { selected: ['bank:js'] })
    expect(wrapper.text()).toContain('3 × Assessment attempts and results')
    expect(button(wrapper, 'Remove these seeds').attributes('disabled')).toBeDefined()
    expect(mocks.invoke.mock.calls.some(c => c[0] === 'dev_seed_reset_run')).toBe(false)
    await wrapper.find('input[type=checkbox]').setValue(true)
    await button(wrapper, 'Remove these seeds').trigger('click'); await flushPromises()
    expect(mocks.invoke).toHaveBeenCalledWith('dev_seed_reset_run', { selected: ['bank:js'], token: 'reviewed-data' })
    expect(wrapper.text()).toContain('1 removed')
  })
  it('requires a fresh review if data changes before reset', async () => {
    const wrapper = render(); await flushPromises()
    await button(wrapper, 'Reset all seeds').trigger('click'); await flushPromises()
    await wrapper.find('input[type=checkbox]').setValue(true)
    mocks.invoke.mockRejectedValueOnce('Affected data changed. Review the reset again before continuing.')
    await button(wrapper, 'Remove these seeds').trigger('click'); await flushPromises()
    expect(wrapper.text()).toContain('Affected data changed')
    expect(wrapper.findAll('button').some(b => b.text() === 'Remove these seeds')).toBe(false)
  })

})
