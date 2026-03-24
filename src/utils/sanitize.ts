import DOMPurify from 'dompurify'

/**
 * Sanitize untrusted HTML content (e.g. course text from IPFS).
 * Strips scripts, event handlers, and other XSS vectors.
 */
export function sanitizeHtml(raw: string): string {
  return DOMPurify.sanitize(raw, { USE_PROFILES: { html: true } })
}

/**
 * Sanitize untrusted SVG content (e.g. course thumbnail_svg).
 * Allows safe SVG elements and filters, strips scripts and event handlers.
 */
export function sanitizeSvg(raw: string): string {
  return DOMPurify.sanitize(raw, { USE_PROFILES: { svg: true, svgFilters: true } })
}
