/** Fallback plugin thumbnail when a plugin ships no icon: a gradient derived
 *  from its CID plus the first letter of its name. Matches Settings → Plugins. */
export function pluginThumbGradient(cid: string): string {
  let h = 0
  for (let i = 0; i < cid.length; i++) h = (h * 31 + cid.charCodeAt(i)) >>> 0
  const a = h % 360
  const b = (a + 60) % 360
  return `linear-gradient(135deg, hsl(${a} 70% 55%), hsl(${b} 70% 45%))`
}

export function pluginMonogram(name: string): string {
  return (name.trim()[0] || '?').toUpperCase()
}
