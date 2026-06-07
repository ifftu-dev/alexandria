// Tests for the Music Reviews pitch-detection helpers. Generates pure
// sine waves at known frequencies and asserts the autocorrelation
// detector picks the correct MIDI semitone (within < 50 cents).

import { describe, it, expect } from 'vitest';
import {
  detectPitch,
  noteToMidi,
  midiToFreq,
  freqToMidiFloat,
  midiToName,
} from './pitch.js';

// Generate `samples` samples of a sine wave at `freq` Hz at `sampleRate`.
function sine(freq, sampleRate, samples, amp = 0.6) {
  const out = new Float32Array(samples);
  for (let i = 0; i < samples; i++) {
    out[i] = amp * Math.sin(2 * Math.PI * freq * i / sampleRate);
  }
  return out;
}

// Two-tone signal — fundamental + a small second harmonic. The
// detector should still lock onto the fundamental.
function harmonic(freq, sampleRate, samples, amp = 0.5, h2 = 0.15) {
  const out = new Float32Array(samples);
  for (let i = 0; i < samples; i++) {
    const t = i / sampleRate;
    out[i] = amp * Math.sin(2 * Math.PI * freq * t) +
             h2 * Math.sin(2 * Math.PI * (freq * 2) * t);
  }
  return out;
}

function noise(samples, amp = 0.4, seed = 1) {
  // Deterministic LCG noise so tests are reproducible.
  const out = new Float32Array(samples);
  let s = seed | 0;
  for (let i = 0; i < samples; i++) {
    s = (s * 1664525 + 1013904223) | 0;
    out[i] = ((s >>> 0) / 0xffffffff - 0.5) * 2 * amp;
  }
  return out;
}

function silence(samples) {
  return new Float32Array(samples); // already zero-filled
}

const SR = 44100;
const FFT = 4096; // matches AnalyserNode.fftSize used by app.js

describe('note utilities', () => {
  it('parses scientific pitch notation', () => {
    expect(noteToMidi('C4')).toBe(60);
    expect(noteToMidi('A4')).toBe(69);
    expect(noteToMidi('A0')).toBe(21);
    expect(noteToMidi('G9')).toBe(127);
  });

  it('handles sharps and flats', () => {
    expect(noteToMidi('F#4')).toBe(66);
    expect(noteToMidi('Gb4')).toBe(66); // enharmonic of F#4
    expect(noteToMidi('Bb3')).toBe(58); // = A#3
    expect(noteToMidi('A#3')).toBe(58);
  });

  it('rejects malformed input', () => {
    expect(noteToMidi('')).toBeNull();
    expect(noteToMidi('H4')).toBeNull();
    expect(noteToMidi('Cb4')).toBeNull(); // flat-of-C requires octave wrap; we reject
    expect(noteToMidi('garbage')).toBeNull();
  });

  it('midiToFreq matches the equal-temperament reference', () => {
    expect(midiToFreq(69)).toBeCloseTo(440, 6);     // A4
    expect(midiToFreq(60)).toBeCloseTo(261.6256, 3); // C4
    expect(midiToFreq(72)).toBeCloseTo(523.2511, 3); // C5
  });

  it('freqToMidiFloat is the inverse of midiToFreq', () => {
    for (const m of [21, 33, 48, 60, 69, 76, 88, 100]) {
      expect(freqToMidiFloat(midiToFreq(m))).toBeCloseTo(m, 6);
    }
  });

  it('midiToName round-trips through noteToMidi', () => {
    for (const name of ['C2', 'D3', 'E4', 'F#4', 'G#5', 'A4', 'B5', 'C6']) {
      const m = noteToMidi(name);
      expect(m).not.toBeNull();
      expect(midiToName(/** @type {number} */ (m))).toBe(name);
    }
  });
});

describe('detectPitch — pure sine waves', () => {
  // 50 cents = half a semitone. The autocorrelation + parabolic refinement
  // should land well inside that tolerance for stable sine input.
  function assertDetectsMidi(targetMidi, name) {
    const freq = midiToFreq(targetMidi);
    const buf = sine(freq, SR, FFT);
    const det = detectPitch(buf, SR);
    expect(det, `${name} detected`).not.toBeNull();
    const midiFloat = freqToMidiFloat(det.freq);
    const cents = Math.abs(midiFloat - targetMidi) * 100;
    expect(cents, `${name} off by ${cents.toFixed(1)}¢`).toBeLessThan(50);
    expect(det.clarity).toBeGreaterThan(0.9);
  }

  it('locks onto C4 (261.63 Hz)', () => assertDetectsMidi(60, 'C4'));
  it('locks onto E4 (329.63 Hz)', () => assertDetectsMidi(64, 'E4'));
  it('locks onto A4 (440 Hz)',    () => assertDetectsMidi(69, 'A4'));
  it('locks onto C5 (523.25 Hz)', () => assertDetectsMidi(72, 'C5'));
  it('locks onto G3 (196 Hz)',    () => assertDetectsMidi(55, 'G3'));
  it('locks onto E2 (82.4 Hz)',   () => assertDetectsMidi(40, 'E2'));
});

describe('detectPitch — robustness', () => {
  it('returns null on pure silence', () => {
    expect(detectPitch(silence(FFT), SR)).toBeNull();
  });

  it('returns null on below-gate amplitude', () => {
    // 0.001 RMS is below the default rmsGate of 0.005.
    const quiet = sine(440, SR, FFT, 0.0005);
    expect(detectPitch(quiet, SR)).toBeNull();
  });

  it('rejects unstructured noise as below-threshold', () => {
    const det = detectPitch(noise(FFT, 0.4, 42), SR);
    // Noise either returns null or flags low clarity — never a confident
    // lock.
    if (det !== null) {
      expect(det.belowThreshold).toBe(true);
      expect(det.clarity).toBeLessThan(0.82);
    }
  });

  it('locks onto the fundamental, not the octave', () => {
    // A4 fundamental with second-harmonic content (typical of voice /
    // bowed strings) — must still return ~440 Hz, not 880 Hz.
    const det = detectPitch(harmonic(440, SR, FFT, 0.5, 0.18), SR);
    expect(det).not.toBeNull();
    const midi = freqToMidiFloat(det.freq);
    expect(Math.abs(midi - 69)).toBeLessThan(0.5); // < 50¢ from A4
  });

  it('honors a custom clarity threshold', () => {
    const buf = sine(440, SR, FFT);
    const lockedSine = detectPitch(buf, SR, { minClarity: 0.99 });
    expect(lockedSine).not.toBeNull();
    expect(lockedSine.belowThreshold).toBe(false);
    // An unreasonably high threshold against noise must flag below-
    // threshold (the detector still returns its best guess so the
    // caller can show "weak signal", but evaluation should ignore it).
    const noisy = detectPitch(noise(FFT, 0.4), SR, { minClarity: 0.99 });
    if (noisy !== null) {
      expect(noisy.belowThreshold).toBe(true);
      expect(noisy.clarity).toBeLessThan(0.99);
    }
  });
});

describe('detectPitch — chromatic scale sweep', () => {
  // For every semitone in a useful vocal/instrument range, the detector
  // must land within < 25 cents of the synthesized frequency. Guards
  // against accidental octave errors over time.
  it('every C2..C6 semitone within 25 cents', () => {
    for (let midi = 36; midi <= 84; midi++) {
      const freq = midiToFreq(midi);
      const buf = sine(freq, SR, FFT);
      const det = detectPitch(buf, SR);
      expect(det, `midi=${midi} not detected`).not.toBeNull();
      const cents = (freqToMidiFloat(det.freq) - midi) * 100;
      expect(Math.abs(cents), `midi=${midi} off ${cents.toFixed(1)}¢`)
        .toBeLessThan(25);
    }
  });
});
