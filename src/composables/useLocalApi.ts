import { invoke as tauriInvoke } from '@tauri-apps/api/core'
import { getProfileSessionToken } from './profileSession'
import profileCommandPolicy from './profile-command-policy.json'
import type { TauriCommand } from '@/generated/tauri-commands'

const unscopedCommands = new Set(Object.keys(profileCommandPolicy.unscoped_commands))

/**
 * Composable that bridges the Vue frontend to the Rust backend via Tauri IPC.
 *
 * Replaces `useFetch` / `$fetch` from Nuxt. Every command corresponds to a
 * `#[tauri::command]` function registered in `src-tauri/src/lib.rs`.
 *
 * Usage:
 *   const { invoke } = useLocalApi()
 *   const wallet = await invoke<WalletInfo>('get_wallet_info')
 *   const courses = await invoke<Course[]>('list_courses', { status: 'published' })
 */
export function useLocalApi() {
  /**
   * Invoke a Tauri command on the Rust backend.
   *
   * @param command - The command name (must match a registered handler)
   * @param args    - Optional arguments object (serialized to JSON)
   * @returns       - The command's return value, deserialized from JSON
   * @throws        - String error message from the Rust side
   */
  async function invoke<T>(command: TauriCommand, args?: Record<string, unknown>): Promise<T> {
    const session = getProfileSessionToken()
    const result = await tauriInvoke<T>(command, args, {
      headers: session ? { 'x-alexandria-profile-session': session } : {},
    })
    if (!unscopedCommands.has(command) && session !== getProfileSessionToken()) {
      throw new Error('Profile session changed before the command completed')
    }
    return result
  }

  return { invoke }
}
