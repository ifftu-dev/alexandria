import { describe, expect, it } from 'vitest'
import router from '@/router'

// `router.resolve` matches a path without navigating, so these assert route
// matching alone and never run the access guard.
describe('unmatched routes', () => {
  // The redirects deleted in 0a42eb0. That commit claimed a stale link would
  // land on a not-found page when no such page existed, so these were the
  // paths that rendered nothing. They now have somewhere honest to go.
  const retired = [
    '/unlock',
    '/instructor/courses/new',
    '/instructor/courses/some-id',
    '/instructor/tutorials/new',
    '/governance',
    '/dashboard/courses',
    '/dashboard/credentials',
    '/dashboard/credentials/some-id',
    '/dashboard/sponsor',
    '/dashboard/reputation',
    '/dashboard/network',
    '/dashboard/sync',
    '/dashboard/sentinel/propose-prior',
  ]

  it('sends every retired deep link to the not-found page', () => {
    for (const path of retired) {
      expect(router.resolve(path).name, path).toBe('not-found')
    }
  })

  // /skills/bootstrap was also retired, but it never reached a catch-all:
  // `/skills/:id` captures it as a skill ID, so it opens the skill page, which
  // shows its own error state for an unknown skill. Pinned here so that
  // behaviour is a known outcome rather than an assumption.
  it('lets /skills/:id handle the retired bootstrap path as an unknown skill', () => {
    const resolved = router.resolve('/skills/bootstrap')
    expect(resolved.name).toBe('skill-detail')
    expect(resolved.params.id).toBe('bootstrap')
  })

  it('sends an arbitrary unknown path to the not-found page', () => {
    expect(router.resolve('/no/such/page').name).toBe('not-found')
  })

  it('does not shadow the routes that do exist', () => {
    expect(router.resolve('/home').name).toBe('home')
    expect(router.resolve('/settings').name).toBe('settings')
    expect(router.resolve('/dashboard/sentinel').name).toBe('dashboard-sentinel')
  })
})
