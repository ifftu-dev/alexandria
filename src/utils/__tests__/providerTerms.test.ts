import { describe, expect, it } from 'vitest'
import { restrictsAppsForMinors } from '../providerTerms'

describe('provider terms notices', () => {
  it('flags the Gemini API and nothing else', () => {
    expect(restrictsAppsForMinors('https://generativelanguage.googleapis.com/v1beta/openai')).toBe(true)
    expect(restrictsAppsForMinors(' https://GENERATIVELANGUAGE.googleapis.com./v1 ')).toBe(true)
    expect(restrictsAppsForMinors('https://api.openai.com/v1')).toBe(false)
    expect(restrictsAppsForMinors('https://evilgenerativelanguage.googleapis.com.example/v1')).toBe(false)
    expect(restrictsAppsForMinors('not a url')).toBe(false)
  })
})
