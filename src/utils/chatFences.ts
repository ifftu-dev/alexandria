// A reply may end with fenced blocks (practice question, memory suggestion,
// proposal) that the backend turns into cards once the answer is complete.
// While it streams, the webview shows only the prose before the first one.

import { REPLY_FENCE_TAGS } from '@/generated/chat-vocab'

const FENCES = REPLY_FENCE_TAGS.map((tag) => '```' + tag)

/** The text before the first fenced block a reply may end with. */
export function proseBeforeFences(text: string): string {
  const starts = FENCES.map((fence) => text.indexOf(fence)).filter((i) => i >= 0)
  return starts.length ? text.slice(0, Math.min(...starts)).trimEnd() : text
}
