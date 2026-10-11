import { describe, expect, it } from 'vitest'
import { provenBloomBySkill } from '../useSkillGraphState'
import { bloomName } from '@/utils/bloom'
import type { VerifiableCredential } from '@/types'

const vc = (subject: string, skillId: string, level: number) =>
  ({ credentialSubject: { id: subject, skillId, level, score: 1 } }) as unknown as VerifiableCredential

describe('provenBloomBySkill', () => {
  it('keeps the highest Bloom rank per skill for the subject', () => {
    const map = provenBloomBySkill(
      [vc('did:me', 'skill_a', 0), vc('did:me', 'skill_a', 3), vc('did:other', 'skill_b', 5), vc('did:me', 'skill_c', 9)],
      'did:me',
    )
    expect(map.get('skill_a')).toBe(3)
    expect(map.has('skill_b')).toBe(false)
    expect(map.get('skill_c')).toBe(5)
    expect(bloomName(map.get('skill_a')!)).toBe('analyze')
    expect(bloomName(0)).toBe('remember')
  })
})
