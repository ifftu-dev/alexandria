// Bootstrap the current skill graph from an uploaded resume / transcript /
// credential. The file is stored in the content store (for evidence), its text
// is matched on-device into skill suggestions the user confirms, and each
// confirmed skill becomes a self-asserted VC with a provenance tier.

import { useLocalApi } from './useLocalApi'
import type { SkillSuggestion } from '@/types'

export type DocType = 'resume' | 'transcript' | 'accredited_credential'

export function useSkillBootstrap() {
  const { invoke } = useLocalApi()

  /** Store the raw file bytes in the content store; returns its BLAKE3 hash. */
  async function uploadEvidence(file: File): Promise<string> {
    const buf = await file.arrayBuffer()
    const { hash } = await invoke<{ hash: string }>('content_add', {
      data: Array.from(new Uint8Array(buf)),
    })
    return hash
  }

  /** Extract candidate skills from document text (suggestions only). */
  function extract(text: string): Promise<SkillSuggestion[]> {
    return invoke<SkillSuggestion[]>('bootstrap_extract', { text })
  }

  /** Claim the confirmed skills as self-asserted VCs; returns the count. */
  function confirm(skillIds: string[], docType: DocType, contentHash?: string): Promise<number> {
    return invoke<number>('bootstrap_confirm', {
      skillIds,
      docType,
      contentHash: contentHash ?? null,
    })
  }

  return { uploadEvidence, extract, confirm }
}
