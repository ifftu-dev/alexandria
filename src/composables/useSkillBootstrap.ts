// Bootstrap the current skill graph from an uploaded resume / transcript /
// credential. The file is stored in the content store (for evidence), its text
// is matched on-device into skill suggestions the user confirms, and each
// confirmed skill becomes a self-asserted VC with a provenance tier.

import { useLocalApi } from './useLocalApi'
import type { SkillSuggestion } from '@/types'

export type DocType = 'resume' | 'transcript' | 'accredited_credential'

export interface PickedFile {
  path: string
  hash: string
  size: number
  /** Decoded text (empty for binary formats like PDF). */
  text: string
}

export function useSkillBootstrap() {
  const { invoke } = useLocalApi()

  /** Open the native OS file picker; returns the selected path (or null). */
  async function pickFile(): Promise<string | null> {
    const { open } = await import('@tauri-apps/plugin-dialog')
    const selected = await open({
      multiple: false,
      directory: false,
      title: 'Choose a resume, transcript, or credential',
    })
    return typeof selected === 'string' ? selected : null
  }

  /**
   * Read a picked file via the scope-checked fs plugin, store its bytes in the
   * content store (evidence), and decode text when it's plain text. Reads are
   * gated by the fs capability scope — the app never exposes arbitrary-path
   * read over IPC.
   */
  async function readFile(path: string): Promise<PickedFile> {
    const { readFile: fsReadFile } = await import('@tauri-apps/plugin-fs')
    const bytes = await fsReadFile(path) // Uint8Array, scope-enforced
    const data = Array.from(bytes)
    const { hash, size } = await invoke<{ hash: string; size: number }>('content_add', { data })
    // Extract text on-device (PDFs parsed server-side; text passed through).
    // Empty ⇒ scanned/image doc; the user pastes the text in that case.
    let text = ''
    try {
      text = await invoke<string>('bootstrap_extract_text', { data })
    } catch {
      /* unreadable (e.g. scanned PDF) — leave blank, user pastes */
    }
    return { path, hash, size, text }
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

  return { pickFile, readFile, extract, confirm }
}
