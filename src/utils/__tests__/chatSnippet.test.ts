import { describe, expect, it } from 'vitest'
import { snippetParts } from '../chatSnippet'

describe('snippetParts', () => {
  it('marks every match and keeps the text between them', () => {
    expect(snippetParts('…the \u0002base\u0003 rate and \u0002base\u0003line…')).toEqual([
      { text: '…the ', hit: false },
      { text: 'base', hit: true },
      { text: ' rate and ', hit: false },
      { text: 'base', hit: true },
      { text: 'line…', hit: false },
    ])
  })

  it('treats stray markers and markup as plain text', () => {
    expect(snippetParts('<b>x</b>')).toEqual([{ text: '<b>x</b>', hit: false }])
    expect(snippetParts('a\u0002b')).toEqual([
      { text: 'a', hit: false },
      { text: 'b', hit: false },
    ])
    expect(snippetParts('\u0002m\u0003\u0003z')).toEqual([
      { text: 'm', hit: true },
      { text: 'z', hit: false },
    ])
    expect(snippetParts('')).toEqual([])
  })
})
