import { invoke as tauriInvoke } from '@tauri-apps/api/core'

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
  async function invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
    return tauriInvoke<T>(command, args)
  }

  return { invoke }
}
