/**
 * The message of anything a `catch` receives: an `Error`'s message, or the
 * value as text (Tauri commands reject with the backend's error string).
 */
export function errorMessage(cause: unknown): string {
  return cause instanceof Error ? cause.message : String(cause)
}
