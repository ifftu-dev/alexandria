import { mount } from '@vue/test-utils'
import { describe, expect, it } from 'vitest'
import PanelIntegrity from './PanelIntegrity.vue'
import type { PanelIntegrity as Integrity } from '@/composables/usePanel'

const render = (integrity: Integrity) => mount(PanelIntegrity, {
  props: { integrity },
  global: {
    mocks: {
      $t: (key: string, values?: Record<string, unknown>) => (values ? `${key} ${JSON.stringify(values)}` : key),
      $te: (key: string) => key === 'sentinel.flags.items.appSwitch.description',
    },
  },
})

describe('PanelIntegrity', () => {
  it('says nothing when the organisation did not ask', () => {
    expect(render({ requested: false, results: [] }).find('[data-testid=panel-integrity]').exists()).toBe(false)
  })

  it('waits for a result, and says whose word it is', () => {
    const text = render({ requested: true, results: [] }).text()
    expect(text).toContain('interviews.panel.integrity.noResult')
    expect(text).toContain('interviews.panel.integrity.waiting')
    expect(text).toContain('interviews.panel.integrity.caveat')
  })

  it('lists the most serious flags first, in Sentinel\'s own words', () => {
    const wrapper = render({
      requested: true,
      results: [{
        outcome: 'flagged', score: 64, startedAt: '2026-10-12T10:00:00Z', endedAt: '2026-10-12T10:30:00Z',
        snapshots: 58, camera: true, cameraCoverage: 97, receivedAt: '2026-10-12T10:30:05Z',
        flags: [
          { flag: 'tab_switching', severity: 'info', count: 4 },
          { flag: 'app_switch', severity: 'warning', count: 2 },
        ],
      }],
    })
    const text = wrapper.text()
    expect(text).toContain('interviews.panel.integrity.flagged')
    expect(text).toContain('{"score":64}')
    expect(text).toContain('{"minutes":30}')
    expect(text).toContain('{"percent":97}')
    const items = wrapper.findAll('li').map(li => li.text())
    expect(items[0]).toContain('sentinel.flags.items.appSwitch.description')
    expect(items[1]).toContain('tab_switching')
  })
})
