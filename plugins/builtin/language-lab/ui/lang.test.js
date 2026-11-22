// Unit tests for the Language Lab pure helpers. The text cases mirror the
// grader's Rust tests so the UI's local diff and tips agree with the score.

import { describe, it, expect } from 'vitest';
import {
  articleFor,
  articleIssues,
  bytesToBase64,
  dictationScore,
  endsWithTerminal,
  extForMime,
  floatToPcm16LE,
  glossarySegments,
  normalize,
  pickVoice,
  readSettings,
  startsWithCapital,
  tokens,
  wordDiff,
  writingTips,
} from './lang.js';

describe('tokens / normalize', () => {
  it('lowercases, strips punctuation and keeps inner apostrophes', () => {
    expect(tokens('  Hello,   WORLD!! ')).toEqual(['hello', 'world']);
    expect(tokens('I\'m here. Don’t go')).toEqual(["i'm", 'here', "don't", 'go']);
    expect(tokens("'quoted' word")).toEqual(['quoted', 'word']);
    expect(normalize('The shop opens at 9.')).toBe('the shop opens at 9');
    expect(tokens(' ...!? ')).toEqual([]);
  });
});

describe('wordDiff', () => {
  it('reports ok / missing / extra like the grader', () => {
    const { matched, ops } = wordDiff(tokens('I drink tea every day'), tokens('I drinks tea every day'));
    expect(matched).toBe(4);
    expect(ops.map((o) => `${o.status}:${o.word}`)).toEqual([
      'ok:i',
      'missing:drink',
      'extra:drinks',
      'ok:tea',
      'ok:every',
      'ok:day',
    ]);
  });

  it('scores dictation as matched / max(expected, typed)', () => {
    expect(dictationScore('Good morning.', 'good morning')).toBe(1);
    expect(dictationScore('I drink tea every day.', 'I drinks tea every day')).toBeCloseTo(0.8);
    expect(dictationScore('Good morning.', 'good good morning morning')).toBe(0.5);
    expect(dictationScore('Good morning.', '')).toBe(0);
  });
});

describe('writing checks', () => {
  it('capital and terminal punctuation', () => {
    expect(startsWithCapital('  "My name is Ravi."')).toBe(true);
    expect(startsWithCapital('my name')).toBe(false);
    expect(startsWithCapital('9 am is early.')).toBe(true);
    expect(endsWithTerminal('Is it raining?  ')).toBe(true);
    expect(endsWithTerminal('He said "yes."')).toBe(true);
    expect(endsWithTerminal('No full stop')).toBe(false);
  });

  it('a/an follows spelling with exceptions', () => {
    expect(articleFor('apple')).toBe('an');
    expect(articleFor('hour')).toBe('an');
    expect(articleFor('university')).toBe('a');
    expect(articleFor('one')).toBe('a');
    expect(articleFor('umbrella')).toBe('an');
    expect(articleFor('8')).toBe(null);
    expect(articleIssues('I have a umbrella and an book, an hour, a university.')).toEqual([
      { article: 'a', next: 'umbrella', expected: 'an' },
      { article: 'an', next: 'book', expected: 'a' },
    ]);
  });

  it('live tips list every failing check, none for empty text', () => {
    expect(writingTips('')).toEqual([]);
    expect(writingTips('I have an umbrella.')).toEqual([]);
    expect(writingTips('i have a apple').map((t) => t.kind)).toEqual(['capital', 'end_punctuation', 'articles']);
  });
});

describe('audio helpers', () => {
  it('converts floats to clamped little-endian int16', () => {
    const bytes = floatToPcm16LE(new Float32Array([0, 1, -1, 0.5, 2, -2, NaN]));
    const view = new DataView(bytes.buffer);
    const samples = Array.from({ length: 7 }, (_, i) => view.getInt16(i * 2, true));
    expect(samples).toEqual([0, 32767, -32768, 16384, 32767, -32768, 0]);
    // Little-endian byte order: 32767 = 0xFF 0x7F.
    expect([bytes[2], bytes[3]]).toEqual([0xff, 0x7f]);
  });

  it('base64-encodes large buffers in chunks', () => {
    const big = new Uint8Array(100_000).map((_, i) => i % 256);
    const b64 = bytesToBase64(big);
    const back = Uint8Array.from(atob(b64), (c) => c.charCodeAt(0));
    expect(back).toEqual(big);
    expect(bytesToBase64(new Uint8Array([104, 105]))).toBe('aGk=');
  });

  it('maps recorder mime types to file extensions', () => {
    expect(extForMime('audio/webm;codecs=opus')).toBe('webm');
    expect(extForMime('audio/mp4')).toBe('m4a');
    expect(extForMime('audio/ogg')).toBe('ogg');
    expect(extForMime('')).toBe('audio');
  });
});

describe('voices, settings and glossary', () => {
  it('prefers en-IN, then en-US, then any English', () => {
    const v = (lang) => ({ lang, name: lang });
    expect(pickVoice([v('hi-IN'), v('en-US'), v('en-IN')]).lang).toBe('en-IN');
    expect(pickVoice([v('hi-IN'), v('en_US'), v('en-GB')]).lang).toBe('en_US');
    expect(pickVoice([v('fr-FR'), v('en-GB')]).lang).toBe('en-GB');
    expect(pickVoice([v('hi-IN')])).toBe(null);
    expect(pickVoice(undefined)).toBe(null);
  });

  it('reads settings defensively', () => {
    expect(readSettings(undefined)).toEqual({ ui_language: 'hi', speech_rate: 0.9, show_hints: true });
    expect(readSettings({ ui_language: 'en', speech_rate: 3, show_hints: false })).toEqual({
      ui_language: 'en',
      speech_rate: 1.5,
      show_hints: false,
    });
    expect(readSettings({ ui_language: 'fr', speech_rate: 'x' }).ui_language).toBe('hi');
  });

  it('marks glossary words case-insensitively and keeps the rest verbatim', () => {
    const segs = glossarySegments('My Family lives here.', { family: 'परिवार', here: 'यहाँ' });
    expect(segs.map((s) => s.text).join('')).toBe('My Family lives here.');
    expect(segs.filter((s) => s.meaning).map((s) => [s.text, s.meaning])).toEqual([
      ['Family', 'परिवार'],
      ['here', 'यहाँ'],
    ]);
  });
});
