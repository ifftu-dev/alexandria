import { describe, it, expect } from 'vitest'
import { isInAppPath, parseDeepLink } from './parse'

describe('parseDeepLink', () => {
  describe('interview panel', () => {
    const link =
      'alexandria://interview/join?cloud=https%3A%2F%2Fcloud.example&id=i-1&panelist=p-1&token=t0k3n'
    it('hands the whole custom-scheme link to the backend', () => {
      expect(parseDeepLink(link)).toEqual({ kind: 'interview-panel', link })
    })
    it('refuses the https form, which would put the token in logs', () => {
      expect(
        parseDeepLink(link.replace('alexandria://interview', 'https://alexandria.ifftu.dev/interview')),
      ).toBeNull()
    })
    it('refuses a link missing any part', () => {
      expect(parseDeepLink(link.replace('&token=t0k3n', ''))).toBeNull()
      expect(parseDeepLink('alexandria://interview/leave?id=i-1')).toBeNull()
    })
  })
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

  describe('governance genesis locator', () => {
    it('routes custom and official links to explicit review', () => {
      const dao = 'ab'.repeat(32)
      const content = 'cd'.repeat(32)
      const query = `content=${content}&source=iroh%3A%2F%2F${content}&source=https%3A%2F%2Fmirror.example%2Fgenesis.json%23blake3%3D${content}`
      const custom = `alexandria://governance/genesis/${dao}?${query}`
      expect(parseDeepLink(custom)).toEqual({
        kind: 'governance-genesis-locator',
        locator: custom,
      })

      const official = `https://alexandria.ifftu.dev/governance/genesis/${dao}?${query}`
      expect(parseDeepLink(official)).toEqual({
        kind: 'governance-genesis-locator',
        locator: official,
      })
    })

    it('rejects a missing DAO id and an oversized locator', () => {
      expect(parseDeepLink('alexandria://governance/genesis')).toBeNull()
      expect(
        parseDeepLink(`alexandria://governance/genesis/${'a'.repeat(64)}?${'x'.repeat(2048)}`),
      ).toBeNull()
    })
  })

  describe('notifications', () => {
    it('has no public notification links, in either form', () => {
      for (const link of [
        'alexandria://notifications',
        'alexandria://notification/n-1',
        'https://alexandria.ifftu.dev/notifications',
        'https://alexandria.ifftu.dev/notification/n-1',
      ]) {
        expect(parseDeepLink(link), link).toBeNull()
      }
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
    it('rejects a backslash route a browser would read as another origin', () => {
      expect(parseDeepLink('alexandria://open?route=%2F%5Cevil.com%2Fx')).toBeNull()
    })
  })

  describe('isInAppPath', () => {
    it('accepts same-document absolute paths', () => {
      for (const path of ['/', '/skills/rust', '/credentials/1?tab=proof#top', '/guardian?accept=a%2Fb']) {
        expect(isInAppPath(path), path).toBe(true)
      }
    })
    it('rejects anything that could leave the app or is not a path', () => {
      for (const path of [
        '', null, undefined, 'skills', ' /skills', '//evil.example/x', '/\\evil.example', '\\/evil.example',
        '/\tevil', '/\n/evil.example', 'javascript:alert(1)', '/javascript:alert(1)\u0000',
        'https://evil.example', 'alexandria://open?route=/x',
      ]) {
        expect(isInAppPath(path), JSON.stringify(path)).toBe(false)
      }
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
