import { mount } from '@vue/test-utils'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { nextTick, reactive } from 'vue'
import SentinelLiveIndicator from './SentinelLiveIndicator.vue'

const debug = reactive({
  active: true,
  suspended: false,
  cameraOptedIn: false,
  faceCount: 0,
  facePresent: false,
  gazeOffscreenRatio: null as number | null,
  appFocusLostCount: 0,
  splitScreen: false,
  screenCaptured: false,
  externalDisplay: false,
  overlaySuspicious: [] as string[],
  watchedProcesses: [] as string[],
  phantomHotkeys: [] as string[],
  cameraDeviceVirtual: false,
  livenessSpoofRatio: null as number | null,
})
vi.mock('vue-i18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }))
vi.mock('@/composables/useSentinel', () => ({ useSentinel: () => ({ debug }) }))

beforeEach(() => {
  debug.active = true
  debug.suspended = false
  debug.appFocusLostCount = 0
})

describe('Sentinel live indicator', () => {
  it('reads nominal only while the session is still being checked', async () => {
    const wrapper = mount(SentinelLiveIndicator)
    expect(wrapper.text()).toContain('sentinel.evidence.liveOk')

    debug.suspended = true
    debug.appFocusLostCount = 1
    await nextTick()
    expect(wrapper.get('.sli-label').text()).toBe('sentinel.evidence.liveSuspended')
    expect(wrapper.text()).not.toContain('sentinel.evidence.liveOk')
  })
})
