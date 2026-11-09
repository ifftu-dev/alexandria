import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { CREDENTIALS_CHANGED } from './credentialEvents'
import { useCredentials } from './useCredentials'
import { useSkillBootstrap } from './useSkillBootstrap'
import { useSponsor } from './useSponsor'
import type { IssueCredentialRequest } from '@/types'

// Every path that issues the learner a credential announces it, so the skill
// atlas (which no longer refreshes on route changes) recounts.

const invoke = vi.hoisted(() => vi.fn<(command: string, args?: Record<string, unknown>) => Promise<unknown>>())
vi.mock('./useLocalApi', () => ({ useLocalApi: () => ({ invoke }) }))

const announced = vi.fn()
beforeEach(() => {
  invoke.mockReset()
  window.addEventListener(CREDENTIALS_CHANGED, announced)
})
afterEach(() => {
  window.removeEventListener(CREDENTIALS_CHANGED, announced)
  announced.mockReset()
})

const request = { credential_type: 'SkillCredential' } as unknown as IssueCredentialRequest

describe('useCredentials.issue', () => {
  it('announces once the credential is issued', async () => {
    invoke.mockResolvedValue({ id: 'urn:uuid:vc-1' })
    await useCredentials().issue(request)
    expect(invoke).toHaveBeenCalledWith('issue_credential', { req: request })
    expect(announced).toHaveBeenCalledTimes(1)
  })

  it('stays silent when issuing fails', async () => {
    invoke.mockRejectedValue(new Error('no key'))
    const api = useCredentials()
    expect(await api.issue(request)).toBeNull()
    expect(api.error.value).toBe('no key')
    expect(announced).not.toHaveBeenCalled()
  })
})

describe('useSkillBootstrap.confirm', () => {
  it('announces when confirmed skills become credentials', async () => {
    invoke.mockResolvedValue(2)
    expect(await useSkillBootstrap().confirm(['a', 'b'], 'resume', 'hash')).toBe(2)
    expect(invoke).toHaveBeenCalledWith('bootstrap_confirm', { skillIds: ['a', 'b'], docType: 'resume', contentHash: 'hash' })
    expect(announced).toHaveBeenCalledTimes(1)
  })

  it('stays silent when nothing was claimed or the claim fails', async () => {
    invoke.mockResolvedValueOnce(0)
    await useSkillBootstrap().confirm([], 'resume')
    invoke.mockRejectedValueOnce(new Error('locked'))
    await expect(useSkillBootstrap().confirm(['a'], 'resume')).rejects.toThrow('locked')
    expect(announced).not.toHaveBeenCalled()
  })
})

describe('useSponsor.issueRoleCredential', () => {
  it('announces once the role credential is issued, and not on failure', async () => {
    invoke.mockResolvedValueOnce({ id: 'urn:uuid:role-1' })
    await useSponsor().issueRoleCredential('role-1', 'did:key:z6Mk', 'session-1')
    expect(announced).toHaveBeenCalledTimes(1)
    invoke.mockRejectedValueOnce(new Error('not passed'))
    expect(await useSponsor().issueRoleCredential('role-1', 'did:key:z6Mk', 'session-1')).toBeNull()
    expect(announced).toHaveBeenCalledTimes(1)
  })
})
