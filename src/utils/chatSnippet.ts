/**
 * Splits a search snippet into plain and matched runs. The backend wraps each
 * match in U+0002 … U+0003; runs are rendered as text, never as HTML.
 */
export function snippetParts(snippet: string): { text: string; hit: boolean }[] {
  const parts: { text: string; hit: boolean }[] = []
  const push = (text: string, hit: boolean) => {
    if (text) parts.push({ text, hit })
  }
  const [lead, ...rest] = snippet.split('\u0002')
  push(lead ?? '', false)
  for (const piece of rest) {
    const end = piece.indexOf('\u0003')
    if (end < 0) {
      push(piece, false)
      continue
    }
    push(piece.slice(0, end), true)
    push(piece.slice(end + 1).split('\u0003').join(''), false)
  }
  return parts
}
