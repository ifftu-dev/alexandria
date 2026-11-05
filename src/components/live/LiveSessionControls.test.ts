import { mount } from '@vue/test-utils'
import { expect, test } from 'vitest'
import { i18n } from '@/i18n'
import LiveSessionControls from './LiveSessionControls.vue'

// jsdom does not apply main.css, so this checks the opt-in contract: the
// call toggles take the shared press feedback, and a press reaches the call
// at once. The desktop UI suite proves the cascade in Chromium and WebKit.

test('microphone, camera and screen toggles take the shared press feedback and act on the first click', async () => {
  const controls = mount(LiveSessionControls, { props: { audioEnabled: true, videoEnabled: false }, global: { plugins: [i18n] } })
  const toggles = ['Mute microphone', 'Turn camera on', 'Share screen'].map(name => controls.get(`button[title="${name}"]`))
  for (const toggle of toggles) expect(toggle.classes()).toContain('pressable')

  for (const toggle of toggles) await toggle.trigger('click')
  expect(Object.keys(controls.emitted())).toEqual(expect.arrayContaining(['toggleAudio', 'toggleVideo', 'toggleScreenShare']))
  expect(controls.emitted('toggleAudio')).toHaveLength(1)
})
