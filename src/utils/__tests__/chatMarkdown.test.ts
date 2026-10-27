import { describe, expect, it } from 'vitest'
import { renderChatMarkdown } from '../chatMarkdown'
import type { ChatSourceRef } from '@/types'

const labels = { unverified: 'Not from your sources', unverifiedHint: 'hint', sourceHint: 'open' }
const sources: ChatSourceRef[] = [
  { id: 'S1', kind: 'lesson', label: 'Conditional probability', course_id: 'c', element_id: 'e' },
]

function render(text: string) {
  const div = document.createElement('div')
  div.innerHTML = renderChatMarkdown(text, sources, labels)
  return div
}

describe('renderChatMarkdown', () => {
  it('turns citations that were sent into chips and flags the rest', () => {
    const div = render('Rare conditions mislead [S1] but not [S7].')
    const verified = div.querySelector('[data-source="S1"]')
    expect(verified?.textContent).toBe('Conditional probability')
    const unverified = div.querySelector('.chat-cite--unverified')
    expect(unverified?.textContent).toBe('Not from your sources')
    expect(unverified?.hasAttribute('data-source')).toBe(false)
  })

  it('shows links as text and never as anchors', () => {
    const div = render('See [the docs](https://evil.example/phish) now.')
    expect(div.querySelector('a')).toBeNull()
    expect(div.textContent).toContain('the docs (https://evil.example/phish)')
  })

  it('drops images and keeps their alt text', () => {
    const div = render('![tracking pixel](https://evil.example/p.png)')
    expect(div.querySelector('img')).toBeNull()
    expect(div.textContent).toContain('[tracking pixel]')
  })

  it('never lets model text become markup', () => {
    const div = render('<script>alert(1)</script><img src=x onerror=alert(1)> [S1]')
    expect(div.querySelector('script')).toBeNull()
    expect(div.querySelector('img')).toBeNull()
    expect(div.querySelectorAll('[onerror]').length).toBe(0)
    // It is shown as the text the model wrote, not run.
    expect(div.textContent).toContain('<script>alert(1)</script>')
  })

  it('leaves citation-like text inside code alone', () => {
    const div = render('`arr[S1]` and [S1]')
    expect(div.querySelector('code')?.textContent).toBe('arr[S1]')
    expect(div.querySelectorAll('[data-source]').length).toBe(1)
  })

  it('a label cannot inject markup through a source title', () => {
    const div = document.createElement('div')
    div.innerHTML = renderChatMarkdown('[S1]', [{ ...sources[0]!, label: '<b>x</b>' }], labels)
    expect(div.querySelector('b')).toBeNull()
    expect(div.textContent).toBe('<b>x</b>')
  })
})
