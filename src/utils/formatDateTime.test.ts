import { describe, expect, it } from 'vitest'
import { formatDateTime } from './formatDateTime'

describe('formatDateTime', () => {
  it('writes a timestamp as a medium date and short time in the locale', () => {
    expect(formatDateTime('2026-11-20T09:01:00.000Z', 'en-GB')).toMatch(/^20 Nov 2026, \d{2}:\d{2}$/)
  })

  it('leaves text that is not a timestamp as it is, and nothing as nothing', () => {
    expect(formatDateTime('soon', 'en')).toBe('soon')
    expect(formatDateTime(null, 'en')).toBe('')
  })
})
