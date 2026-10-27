import { renderMarkdown } from './markdown'
import type { ChatSourceRef } from '@/types'

/**
 * Renders a model's answer for the chat panel.
 *
 * Starts from the app's sanitized Markdown, then tightens it for content a
 * model wrote:
 *
 * - Links become plain text showing their address. A model can produce any
 *   URL; clicking one should never be a single step away.
 * - Images are replaced by their alt text. The CSP already blocks remote
 *   images; this keeps the layout honest about it.
 * - `[S1]`-style citations become chips. A citation is verified only when
 *   that identifier was one of the sources actually sent with the question;
 *   anything else is marked as not from the learner's sources.
 *
 * All rewriting happens on parsed DOM nodes, never by string splicing, so
 * nothing the model wrote can reach the page as markup.
 */
export function renderChatMarkdown(
  text: string,
  sources: ChatSourceRef[],
  labels: { unverified: string; unverifiedHint: string; sourceHint: string },
): string {
  const html = renderMarkdown(text)
  const doc = new DOMParser().parseFromString(`<div>${html}</div>`, 'text/html')
  const root = doc.body.firstElementChild as HTMLElement

  for (const img of Array.from(root.querySelectorAll('img'))) {
    const alt = img.getAttribute('alt') ?? ''
    img.replaceWith(doc.createTextNode(alt ? `[${alt}]` : ''))
  }
  for (const link of Array.from(root.querySelectorAll('a'))) {
    const label = link.textContent ?? ''
    const href = link.getAttribute('href') ?? ''
    const span = doc.createElement('span')
    span.className = 'chat-link'
    span.textContent = href && href !== label ? `${label} (${href})` : label
    link.replaceWith(span)
  }

  const byId = new Map(sources.map((s) => [s.id, s]))
  const walker = doc.createTreeWalker(root, NodeFilter.SHOW_TEXT)
  const textNodes: Text[] = []
  while (walker.nextNode()) {
    const node = walker.currentNode as Text
    // Leave code exactly as written.
    if (node.parentElement?.closest('code, pre')) continue
    if (/\[S\d+\]/.test(node.data)) textNodes.push(node)
  }
  for (const node of textNodes) {
    const parts = node.data.split(/(\[S\d+\])/)
    const fragment = doc.createDocumentFragment()
    for (const part of parts) {
      const match = /^\[(S\d+)\]$/.exec(part)
      if (!match) {
        if (part) fragment.append(doc.createTextNode(part))
        continue
      }
      const source = byId.get(match[1]!)
      const chip = doc.createElement('span')
      if (source) {
        chip.className = 'chat-cite'
        chip.setAttribute('role', 'link')
        chip.setAttribute('tabindex', '0')
        chip.setAttribute('data-source', source.id)
        chip.setAttribute('title', labels.sourceHint)
        chip.textContent = source.label
      } else {
        chip.className = 'chat-cite chat-cite--unverified'
        chip.setAttribute('title', labels.unverifiedHint)
        chip.textContent = labels.unverified
      }
      fragment.append(chip)
    }
    node.replaceWith(fragment)
  }
  return root.innerHTML
}
