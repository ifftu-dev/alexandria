import { describe, expect, it } from 'vitest'
import { REPLY_FENCE_TAGS } from '@/generated/chat-vocab'
import { proseBeforeFences } from '../chatFences'

describe('streaming fence stripper', () => {
  it('hides every kind of fenced block the backend turns into a card', () => {
    for (const tag of REPLY_FENCE_TAGS) {
      const reply = `Here is the answer.\n\n\`\`\`${tag}\n{"kind":"x"}`
      expect(proseBeforeFences(reply), tag).toBe('Here is the answer.')
    }
  })

  it('includes the artifact fence, which used to flash as raw text', () => {
    expect(REPLY_FENCE_TAGS).toContain('artifact')
    expect(proseBeforeFences('A note.\n```artifact\n{"type":"note"')).toBe('A note.')
  })

  it('cuts at the earliest fence and leaves plain prose alone', () => {
    expect(proseBeforeFences('One\n```memory\n{}\n```\n```practice\n{}')).toBe('One')
    expect(proseBeforeFences('Plain text with ```js code``` inside')).toBe('Plain text with ```js code``` inside')
    expect(proseBeforeFences('')).toBe('')
  })
})
