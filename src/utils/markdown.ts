import DOMPurify from 'dompurify'

/**
 * Minimal, dependency-light Markdown → sanitized HTML renderer for plugin
 * README docs. Supports headings, bold/italic/inline-code, fenced + inline
 * code, links, images, unordered/ordered lists, blockquotes, hr, and
 * paragraphs — enough for a plugin author's docs without pulling a full
 * Markdown engine into the bundle.
 *
 * Relative image sources (e.g. `screenshots/play.png`) are emitted with an
 * empty `src` and a `data-rel` attribute carrying the original path, so the
 * caller can resolve them to `data:` URLs asynchronously (the main window's
 * CSP forbids the `plugin://` scheme for `<img>`). Absolute/`data:`/`http`
 * image sources pass through unchanged.
 */
export function renderMarkdown(src: string): string {
  const lines = src.replace(/\r\n/g, '\n').split('\n')
  const out: string[] = []
  let i = 0
  let inCode = false
  let codeBuf: string[] = []
  let listType: 'ul' | 'ol' | null = null

  const closeList = () => {
    if (listType) {
      out.push(`</${listType}>`)
      listType = null
    }
  }

  const inline = (t: string): string => {
    // Escape first; we re-introduce a controlled tag set below.
    let s = t
      .replace(/&/g, '&amp;')
      .replace(/</g, '&lt;')
      .replace(/>/g, '&gt;')
    // images ![alt](src)
    s = s.replace(/!\[([^\]]*)\]\(([^)\s]+)\)/g, (_m, alt: string, url: string) => {
      const isAbsolute = /^(https?:|data:|blob:|plugin:)/i.test(url)
      if (isAbsolute) {
        return `<img alt="${alt}" src="${url}" loading="lazy" />`
      }
      // Relative — resolve later via data-rel.
      return `<img alt="${alt}" src="" data-rel="${url}" loading="lazy" />`
    })
    // links [text](href)
    s = s.replace(/\[([^\]]+)\]\(([^)\s]+)\)/g, (_m, text: string, href: string) => {
      return `<a href="${href}" target="_blank" rel="noopener noreferrer">${text}</a>`
    })
    // inline code
    s = s.replace(/`([^`]+)`/g, (_m, c: string) => `<code>${c}</code>`)
    // bold
    s = s.replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>')
    s = s.replace(/__([^_]+)__/g, '<strong>$1</strong>')
    // italic
    s = s.replace(/(^|[^*])\*([^*]+)\*/g, '$1<em>$2</em>')
    s = s.replace(/(^|[^_])_([^_]+)_/g, '$1<em>$2</em>')
    return s
  }

  while (i < lines.length) {
    const line = lines[i] ?? ''

    // Fenced code blocks.
    const fence = line.match(/^```(.*)$/)
    if (fence) {
      if (inCode) {
        out.push(`<pre><code>${codeBuf.join('\n')
          .replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')}</code></pre>`)
        codeBuf = []
        inCode = false
      } else {
        closeList()
        inCode = true
      }
      i++
      continue
    }
    if (inCode) {
      codeBuf.push(line)
      i++
      continue
    }

    // Blank line.
    if (/^\s*$/.test(line)) {
      closeList()
      i++
      continue
    }

    // Headings.
    const h = line.match(/^(#{1,6})\s+(.*)$/)
    if (h) {
      closeList()
      const level = (h[1] ?? '').length
      out.push(`<h${level}>${inline(h[2] ?? '')}</h${level}>`)
      i++
      continue
    }

    // Horizontal rule.
    if (/^(-{3,}|\*{3,}|_{3,})\s*$/.test(line)) {
      closeList()
      out.push('<hr />')
      i++
      continue
    }

    // Blockquote.
    if (/^>\s?/.test(line)) {
      closeList()
      out.push(`<blockquote>${inline(line.replace(/^>\s?/, ''))}</blockquote>`)
      i++
      continue
    }

    // Unordered list.
    const ul = line.match(/^\s*[-*+]\s+(.*)$/)
    if (ul) {
      if (listType !== 'ul') { closeList(); out.push('<ul>'); listType = 'ul' }
      out.push(`<li>${inline(ul[1] ?? '')}</li>`)
      i++
      continue
    }

    // Ordered list.
    const ol = line.match(/^\s*\d+\.\s+(.*)$/)
    if (ol) {
      if (listType !== 'ol') { closeList(); out.push('<ol>'); listType = 'ol' }
      out.push(`<li>${inline(ol[1] ?? '')}</li>`)
      i++
      continue
    }

    // Paragraph (greedy: join following non-blank, non-structural lines).
    closeList()
    const para: string[] = [line]
    i++
    while (i < lines.length) {
      const next = lines[i] ?? ''
      if (/^\s*$/.test(next) ||
          /^(#{1,6}\s|```|>\s?|\s*[-*+]\s|\s*\d+\.\s|(-{3,}|\*{3,}|_{3,})\s*$)/.test(next)) {
        break
      }
      para.push(next)
      i++
    }
    out.push(`<p>${inline(para.join(' '))}</p>`)
  }

  if (inCode && codeBuf.length) {
    out.push(`<pre><code>${codeBuf.join('\n')}</code></pre>`)
  }
  closeList()

  return DOMPurify.sanitize(out.join('\n'), {
    ALLOWED_TAGS: [
      'h1', 'h2', 'h3', 'h4', 'h5', 'h6', 'p', 'a', 'strong', 'em', 'code',
      'pre', 'ul', 'ol', 'li', 'blockquote', 'hr', 'img', 'br',
    ],
    ALLOWED_ATTR: ['href', 'target', 'rel', 'src', 'alt', 'loading', 'data-rel'],
  })
}
