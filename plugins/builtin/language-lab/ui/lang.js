// Pure helpers for Language Lab, kept free of DOM and `window.alex` so they
// can be unit-tested (see lang.test.js). The text rules mirror the grader
// (plugins/builtin/language-lab-grader/src/text.rs) so local hints and the
// dictation diff agree with the score.

/** Lowercase words with punctuation removed; inner apostrophes kept. */
export function tokens(text) {
  const cleaned = String(text ?? '')
    .replace(/[‘’`]/g, "'")
    .toLowerCase()
    .replace(/[^\p{L}\p{N}']+/gu, ' ');
  return cleaned
    .split(/\s+/)
    .map((w) => w.replace(/^'+|'+$/g, ''))
    .filter((w) => w.length > 0);
}

export function normalize(text) {
  return tokens(text).join(' ');
}

/**
 * Word-level LCS diff. Returns `{ matched, ops: [{ word, status }] }` with
 * status `ok` | `missing` (expected, not typed) | `extra` (typed, not
 * expected). Same tie-break as the grader.
 */
export function wordDiff(expected, typed) {
  const n = expected.length;
  const m = typed.length;
  const dp = Array.from({ length: n + 1 }, () => new Uint32Array(m + 1));
  for (let i = n - 1; i >= 0; i--) {
    for (let j = m - 1; j >= 0; j--) {
      dp[i][j] =
        expected[i] === typed[j]
          ? dp[i + 1][j + 1] + 1
          : Math.max(dp[i + 1][j], dp[i][j + 1]);
    }
  }
  const ops = [];
  let i = 0;
  let j = 0;
  while (i < n && j < m) {
    if (expected[i] === typed[j]) {
      ops.push({ word: expected[i], status: 'ok' });
      i++;
      j++;
    } else if (dp[i + 1][j] >= dp[i][j + 1]) {
      ops.push({ word: expected[i++], status: 'missing' });
    } else {
      ops.push({ word: typed[j++], status: 'extra' });
    }
  }
  while (i < n) ops.push({ word: expected[i++], status: 'missing' });
  while (j < m) ops.push({ word: typed[j++], status: 'extra' });
  return { matched: dp[0][0], ops };
}

/** Item score the grader gives a dictation answer (0..1). */
export function dictationScore(expectedText, typedText) {
  const e = tokens(expectedText);
  const t = tokens(typedText);
  const den = Math.max(e.length, t.length);
  if (den === 0) return 0;
  return wordDiff(e, t).matched / den;
}

export function startsWithCapital(text) {
  const first = String(text ?? '').match(/[\p{L}\p{N}]/u);
  if (!first) return false;
  const c = first[0];
  if (!/\p{L}/u.test(c)) return true;
  return c === c.toUpperCase() && c !== c.toLowerCase();
}

export function endsWithTerminal(text) {
  const trimmed = String(text ?? '').replace(/[\s"')”’]+$/u, '');
  return /[.?!]$/.test(trimmed);
}

const A_PREFIXES = ['univers', 'uniform', 'unit', 'union', 'uniq', 'use', 'usu', 'europ', 'eulog', 'ewe'];
const A_EXACT = ['one', 'once', 'u'];
const AN_PREFIXES = ['hour', 'honest', 'honour', 'honor', 'heir'];

/** "a" | "an" | null (no opinion, e.g. a number). */
export function articleFor(word) {
  const w = String(word ?? '');
  if (!/^\p{L}/u.test(w)) return null;
  if (A_EXACT.includes(w) || A_PREFIXES.some((p) => w.startsWith(p))) return 'a';
  if (AN_PREFIXES.some((p) => w.startsWith(p))) return 'an';
  return /^[aeiou]/.test(w) ? 'an' : 'a';
}

/** `[{ article, next, expected }]` for every a/an that disagrees with the next word. */
export function articleIssues(text) {
  const words = tokens(text);
  const out = [];
  for (let i = 0; i + 1 < words.length; i++) {
    const article = words[i];
    if (article !== 'a' && article !== 'an') continue;
    const expected = articleFor(words[i + 1]);
    if (expected && expected !== article) out.push({ article, next: words[i + 1], expected });
  }
  return out;
}

/** Live, non-scoring writing tips: which basic checks currently fail. */
export function writingTips(text) {
  const tips = [];
  if (tokens(text).length === 0) return tips;
  if (!startsWithCapital(text)) tips.push({ kind: 'capital' });
  if (!endsWithTerminal(text)) tips.push({ kind: 'end_punctuation' });
  for (const issue of articleIssues(text)) tips.push({ kind: 'articles', ...issue });
  return tips;
}

/** Float samples in [-1, 1] → little-endian signed 16-bit PCM bytes. */
export function floatToPcm16LE(samples) {
  const out = new Uint8Array(samples.length * 2);
  const view = new DataView(out.buffer);
  for (let i = 0; i < samples.length; i++) {
    let s = samples[i];
    if (!(s === s)) s = 0; // NaN
    s = Math.max(-1, Math.min(1, s));
    view.setInt16(i * 2, s < 0 ? Math.round(s * 0x8000) : Math.round(s * 0x7fff), true);
  }
  return out;
}

export function bytesToBase64(bytes) {
  let s = '';
  const chunk = 0x8000;
  for (let i = 0; i < bytes.length; i += chunk) {
    s += String.fromCharCode.apply(null, bytes.subarray(i, i + chunk));
  }
  return btoa(s);
}

/** File extension for a MediaRecorder mime type. */
export function extForMime(mime) {
  const m = String(mime || '').toLowerCase();
  if (m.includes('webm')) return 'webm';
  if (m.includes('ogg')) return 'ogg';
  if (m.includes('mp4') || m.includes('aac') || m.includes('m4a')) return 'm4a';
  if (m.includes('wav')) return 'wav';
  if (m.includes('mpeg')) return 'mp3';
  return 'audio';
}

/** Prefer Indian English, then US English, then any English voice. */
export function pickVoice(voices) {
  const list = Array.isArray(voices) ? voices : [];
  const lang = (v) => String(v.lang || '').toLowerCase().replace('_', '-');
  return (
    list.find((v) => lang(v) === 'en-in') ||
    list.find((v) => lang(v) === 'en-us') ||
    list.find((v) => lang(v).startsWith('en')) ||
    null
  );
}

export const DEFAULT_SETTINGS = Object.freeze({
  ui_language: 'hi',
  speech_rate: 0.9,
  show_hints: true,
});

/** Read plugin settings from an `init` payload, falling back to defaults. */
export function readSettings(raw) {
  const s = raw && typeof raw === 'object' ? raw : {};
  const rate = Number(s.speech_rate);
  return {
    ui_language: s.ui_language === 'en' ? 'en' : 'hi',
    speech_rate: Number.isFinite(rate) ? Math.min(1.5, Math.max(0.5, rate)) : DEFAULT_SETTINGS.speech_rate,
    show_hints: typeof s.show_hints === 'boolean' ? s.show_hints : DEFAULT_SETTINGS.show_hints,
  };
}

/** Split a passage into word and non-word runs, marking glossary words. */
export function glossarySegments(passage, glossary) {
  const map = new Map();
  for (const [k, v] of Object.entries(glossary || {})) {
    if (typeof v === 'string') map.set(String(k).toLowerCase(), v);
  }
  const out = [];
  const re = /[\p{L}\p{N}'’]+/gu;
  let last = 0;
  let m;
  const text = String(passage ?? '');
  while ((m = re.exec(text))) {
    if (m.index > last) out.push({ text: text.slice(last, m.index) });
    const key = m[0].toLowerCase().replace(/’/g, "'");
    const meaning = map.get(key);
    out.push(meaning ? { text: m[0], word: key, meaning } : { text: m[0] });
    last = m.index + m[0].length;
  }
  if (last < text.length) out.push({ text: text.slice(last) });
  return out;
}
