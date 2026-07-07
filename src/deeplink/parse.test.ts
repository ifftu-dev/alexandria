import { describe, it, expect } from 'vitest'
import { parseDeepLink } from './parse'

describe('parseDeepLink', () => {
  describe('guardian accept', () => {
    it('parses the custom scheme with a code', () => {
      expect(parseDeepLink('alexandria://guardian/accept?code=ABC123')).toEqual({
        kind: 'guardian-accept',
        code: 'ABC123',
      })
    })
    it('parses the https app-link form', () => {
      expect(
        parseDeepLink('https://alexandria.ifftu.dev/guardian/accept?code=ABC123'),
      ).toEqual({ kind: 'guardian-accept', code: 'ABC123' })
    })
    it('trims surrounding whitespace on the code', () => {
      expect(parseDeepLink('alexandria://guardian/accept?code=%20xy%20')).toEqual({
        kind: 'guardian-accept',
        code: 'xy',
      })
    })
    it('rejects guardian without accept', () => {
      expect(parseDeepLink('alexandria://guardian/reject?code=X')).toBeNull()
    })
    it('rejects guardian accept without a code', () => {
      expect(parseDeepLink('alexandria://guardian/accept')).toBeNull()
    })
  })

  describe('course + classroom', () => {
    it('maps course to /courses/:id (custom + https)', () => {
      expect(parseDeepLink('alexandria://course/abc-123')).toEqual({
        kind: 'route',
        path: '/courses/abc-123',
      })
      expect(parseDeepLink('https://alexandria.ifftu.dev/course/abc-123')).toEqual({
        kind: 'route',
        path: '/courses/abc-123',
      })
    })
    it('maps classroom to /classrooms/:id', () => {
      expect(parseDeepLink('alexandria://classroom/room9')).toEqual({
        kind: 'route',
        path: '/classrooms/room9',
      })
    })
    it('keeps the already-encoded id segment (no double-escape)', () => {
      expect(parseDeepLink('alexandria://course/a b')).toEqual({
        kind: 'route',
        path: '/courses/a%20b',
      })
    })
    it('rejects course without an id', () => {
      expect(parseDeepLink('alexandria://course')).toBeNull()
    })
  })

  describe('generic open?route', () => {
    it('accepts an absolute in-app path', () => {
      expect(parseDeepLink('alexandria://open?route=/skills/rust')).toEqual({
        kind: 'route',
        path: '/skills/rust',
      })
    })
    it('rejects a protocol-relative path (open-redirect guard)', () => {
      expect(parseDeepLink('alexandria://open?route=//evil.com/x')).toBeNull()
    })
    it('rejects a non-absolute route', () => {
      expect(parseDeepLink('alexandria://open?route=skills')).toBeNull()
    })
    it('rejects a missing route param', () => {
      expect(parseDeepLink('alexandria://open')).toBeNull()
    })
  })

  describe('rejections', () => {
    it('rejects an unknown action', () => {
      expect(parseDeepLink('alexandria://wat/ever')).toBeNull()
    })
    it('rejects an unknown host on https', () => {
      expect(parseDeepLink('https://evil.example/guardian/accept?code=X')).toBeNull()
    })
    it('rejects a foreign scheme', () => {
      expect(parseDeepLink('mailto:someone@example.com')).toBeNull()
    })
    it('rejects garbage', () => {
      expect(parseDeepLink('not a url')).toBeNull()
    })
  })
})
