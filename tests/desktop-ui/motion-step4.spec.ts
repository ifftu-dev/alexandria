import { expect, test, type Locator } from '@playwright/test'
import { MODES, expectMotion, expectPressFeedback, motionAfter, motionOnArrival, open, setMode } from './motionHelpers'

// Rollout step 4: settings, chat, live calls, instructor authoring and
// classrooms, and guardian oversight, each under System, Reduced and Off.

/**
 * Click a control and, on the first frame its `attribute` reads `value`,
 * report the motion running on it. The change itself never waits for motion.
 */
async function motionOnChange(control: Locator, attribute: string, value: string): Promise<number> {
  return control.evaluate((el, [attribute, value]) => new Promise<number>(resolve => {
    (el as HTMLElement).click()
    const check = () => {
      if (el.getAttribute(attribute) !== value) return requestAnimationFrame(check)
      requestAnimationFrame(() => resolve(el.getAnimations({ subtree: true })
        .filter(a => a.playState === 'running' && Number(a.effect?.getComputedTiming().duration) > 1).length))
    }
    check()
  }), [attribute, value] as const)
}

for (const mode of MODES) {
  test(`settings sections and switches respond under the motion setting (${mode})`, async ({ page }) => {
    await open(page, '/settings/account')
    await setMode(page, mode)
    const nav = page.getByRole('navigation', { name: 'Settings' })
    const body = page.locator('[data-settings-body]')

    const system = nav.getByRole('button', { name: /^System/ })
    expectMotion(await motionAfter(system, body), mode)
    await expect(system).toHaveAttribute('aria-current', 'page')
    await expect(page.getByRole('heading', { name: 'System', level: 3 })).toBeVisible()
    await expect(body).toHaveCSS('opacity', '1')

    const notifications = page.getByRole('switch', { name: 'Show notifications' })
    await expect(notifications).toHaveAttribute('aria-checked', 'true')
    expectMotion(await motionAfter(notifications, notifications), mode)
    await expect(notifications).toHaveAttribute('aria-checked', 'false')
    await expectPressFeedback(page, notifications, mode)

    await expectPressFeedback(page, nav.getByRole('button', { name: /^Account/ }), mode)
    await expect(page.getByRole('heading', { name: 'Account', level: 3 })).toBeVisible()
  })
}

for (const mode of MODES) {
  test(`chat messages arrive under the motion setting without holding input or scrolling (${mode})`, async ({ page }) => {
    await open(page, '/chat')
    await setMode(page, mode)
    const input = page.getByRole('textbox', { name: 'Ask a question…' })
    await input.fill('What is a stable sort?')
    const arrival = await motionOnArrival(page, () => input.press('Enter'), '[data-message-id]', 'A stable sort keeps')
    expectMotion(arrival, mode)
    const answer = page.locator('[data-message-id]').filter({ hasText: 'A stable sort keeps' })
    // The conversation follows the answer to the live edge at once, and the
    // composer takes the next question straight away.
    expect(await answer.evaluate(el => {
      const scroller = el.closest('.chat-scroll')!
      return scroller.scrollHeight > scroller.clientHeight && scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight < 8
    })).toBe(true)
    await input.pressSequentially('And merge sort?')
    await expect(input).toHaveValue('And merge sort?')
    await expect(answer).toHaveCSS('opacity', '1')
    await expect(page.locator('[data-message-id]').filter({ hasText: 'What is a stable sort?' })).toHaveCSS('opacity', '1')
  })
}

for (const mode of MODES) {
  test(`live-call controls and call chat respond under the motion setting (${mode})`, async ({ page }) => {
    await open(page, '/tutoring/session-fixture')
    await setMode(page, mode)
    const mic = page.getByRole('button', { name: 'Mute microphone' })
    await expectPressFeedback(page, mic, mode)
    const unmute = page.getByRole('button', { name: 'Unmute microphone' })
    await expect(unmute).toBeVisible()
    expectMotion(await motionOnChange(unmute, 'title', 'Mute microphone'), mode)
    await expectPressFeedback(page, page.getByRole('button', { name: 'Turn on camera' }), mode)
    await expect(page.getByRole('button', { name: 'Turn off camera' })).toBeVisible()

    await page.getByRole('button', { name: 'Chat', exact: true }).click()
    const input = page.getByPlaceholder('Type a message…')
    await input.fill('Is merge sort stable?')
    const arrival = await motionOnArrival(page, () => input.press('Enter'), '[data-call-message]', 'Is merge sort stable?')
    expectMotion(arrival, mode)
    await expect(input).toHaveValue('')
    await input.pressSequentially('Yes')
    await expect(input).toHaveValue('Yes')
    await expect(page.locator('[data-call-message]').filter({ hasText: 'Is merge sort stable?' })).toHaveCSS('opacity', '1')
  })
}

for (const mode of MODES) {
  test(`classroom channels and messages respond under the motion setting (${mode})`, async ({ page }) => {
    await open(page, '/classrooms')
    await setMode(page, mode)
    await expectPressFeedback(page, page.getByRole('link', { name: /Algorithms club/ }), mode)
    await expect(page.locator('output')).toHaveText('/classrooms/classroom-fixture')
    const channel = page.getByRole('button', { name: /general/ })
    await expect(page.locator('[data-classroom-message]')).toHaveCount(2)
    await expectPressFeedback(page, channel, mode)
    await expect(channel).toHaveAttribute('aria-current', 'true')

    const input = page.getByPlaceholder('Message #general')
    await input.fill('Here is my puzzle')
    const arrival = await motionOnArrival(page, () => input.press('Enter'), '[data-classroom-message]', 'Here is my puzzle')
    expectMotion(arrival, mode)
    await expect(input).toHaveValue('')
    await input.pressSequentially('Next one')
    await expect(input).toHaveValue('Next one')
    await expect(page.locator('[data-classroom-message]').filter({ hasText: 'Here is my puzzle' })).toHaveCSS('opacity', '1')
  })
}

for (const mode of MODES) {
  test(`guardian oversight and approvals respond under the motion setting (${mode})`, async ({ page }) => {
    await open(page, '/guardian')
    await setMode(page, mode)
    await expectPressFeedback(page, page.getByRole('button', { name: /Asha/ }), mode)
    await expect(page.locator('output')).toHaveText('/guardian/child/link-fixture')

    const panel = page.locator('[data-guardian-panel]')
    const request = page.locator('[data-ai-request]').filter({ hasText: 'Study helper' })
    await expect(request).toContainText('Waiting for you')
    const tabs = page.getByRole('tablist')
    const courses = tabs.getByRole('tab', { name: 'Courses', exact: true })
    const overview = tabs.getByRole('tab', { name: 'Overview', exact: true })
    await expectPressFeedback(page, courses, mode)
    await expect(courses).toHaveAttribute('aria-selected', 'true')
    await expect(overview).toHaveAttribute('aria-selected', 'false')
    await expect(page.getByText('No enrollments')).toBeVisible()
    expectMotion(await motionAfter(overview, panel), mode)
    await expect(overview).toHaveAttribute('aria-selected', 'true')
    await expect(panel).toHaveCSS('opacity', '1')
    // The arrow keys move focus and selection along the tablist.
    await overview.press('ArrowRight')
    await expect(courses).toBeFocused()
    await expect(courses).toHaveAttribute('aria-selected', 'true')
    await courses.press('ArrowLeft')
    await expect(overview).toBeFocused()

    const decided = await motionOnArrival(page, () => request.getByRole('button', { name: 'Approve' }).click(), '[data-ai-request]', 'Approved')
    expectMotion(decided, mode)
    await expect(request.getByRole('button', { name: 'Withdraw approval' })).toBeVisible()
    await expect(request).toHaveCSS('opacity', '1')
  })
}

for (const mode of MODES) {
  test(`instructor authoring responds under the motion setting (${mode})`, async ({ page }) => {
    await open(page, '/instructor/courses')
    await setMode(page, mode)
    await expectPressFeedback(page, page.getByRole('button', { name: /Sorting algorithms/ }), mode)
    await expect(page.locator('output')).toHaveText('/instructor/composer/course-sorting')

    const stages = page.getByRole('navigation', { name: 'Course creation stages' })
    const content = page.locator('#course-stage-content')
    const brief = stages.getByRole('button', { name: /Brief/ })
    await expectPressFeedback(page, brief, mode)
    await expect(brief).toHaveAttribute('aria-current', 'step')
    const curriculum = stages.getByRole('button', { name: /Curriculum/ })
    expectMotion(await motionAfter(curriculum, content), mode)
    await expect(curriculum).toHaveAttribute('aria-current', 'step')
    await expect(content).toHaveCSS('opacity', '1')

    await expectPressFeedback(page, page.getByRole('button', { name: /Insertion sort/ }), mode)
    await page.getByRole('button', { name: '+ Add element' }).click()
    // The outline's add form comes before the selected lesson's editor.
    const title = page.getByPlaceholder('Element title').first()
    await title.fill('Quick sort')
    const arrival = await motionOnArrival(page, () => title.press('Enter'), '[data-outline-element]', 'Quick sort')
    expectMotion(arrival, mode)
    await expect(page.locator('[data-outline-element]').filter({ hasText: 'Quick sort' })).toHaveCSS('opacity', '1')
  })
}
