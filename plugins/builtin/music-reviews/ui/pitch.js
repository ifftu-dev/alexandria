// Pure pitch-detection + music-theory helpers extracted from app.js so
// they can be unit-tested in isolation. Loaded by the iframe via
// `<script type="module">` in index.html and imported by app.js.

export const NOTE_NAMES = ['C', 'C#', 'D', 'D#', 'E', 'F', 'F#', 'G', 'G#', 'A', 'A#', 'B'];

/** Parse a note name like "A4" or "F#3" into its MIDI number. Flats are
 *  converted to their enharmonic sharp ("Db4" → C#3 spelling rejected
 *  for simplicity; "Bb3" → A#3). Returns null on invalid input. */
export function noteToMidi(name) {
  const m = /^([A-G])(#|b)?(-?\d+)$/.exec(String(name).trim());
  if (!m) return null;
  let pc = m[1] + (m[2] === '#' ? '#' : '');
  if (m[2] === 'b') {
    const i = NOTE_NAMES.indexOf(m[1]);
    if (i <= 0) return null;
    pc = NOTE_NAMES[(i - 1 + 12) % 12];
  }
  const idx = NOTE_NAMES.indexOf(pc);
  if (idx < 0) return null;
  return (parseInt(m[3], 10) + 1) * 12 + idx;
}

export function midiToFreq(midi) {
  return 440 * Math.pow(2, (midi - 69) / 12);
}

export function freqToMidiFloat(freq) {
  return 69 + 12 * Math.log2(freq / 440);
}

export function midiToName(midi) {
  const r = Math.round(midi);
  const pc = ((r % 12) + 12) % 12;
  const oct = Math.floor(r / 12) - 1;
  return `${NOTE_NAMES[pc]}${oct}`;
}

/** Autocorrelation pitch detector. Returns `{ freq, clarity }` for the
 *  strongest periodic component in the window, or null if the signal is
 *  too quiet or non-periodic to call.
 *
 *  - `samples`: Float32Array of time-domain audio in [-1, 1].
 *  - `sampleRate`: e.g. 44100 or 48000.
 *
 *  The algorithm:
 *  1. Reject windows below an RMS gate (silence).
 *  2. Trim leading/trailing low-energy samples to tighten the
 *     correlation window — improves clarity on attack transients.
 *  3. For each lag in [sr/1200, sr/60] (60 Hz–1.2 kHz), compute the
 *     normalized autocorrelation. Track the first peak that exceeds a
 *     correlation threshold and falls back below — that lag is the
 *     fundamental period.
 *  4. Parabolic interpolation around the integer-lag peak for
 *     sub-sample frequency accuracy. */
export function detectPitch(samples, sampleRate, opts) {
  const SIZE = samples.length;
  if (SIZE === 0) return null;
  const o = opts || {};
  const minClarity = typeof o.minClarity === 'number' ? o.minClarity : 0.82;
  const rmsGate = typeof o.rmsGate === 'number' ? o.rmsGate : 0.0025;
  // Default 0 disables trimming — real-world mic input often has a DC
  // bias and low peaks, so chopping to first |x|>=t can leave too few
  // samples for the long-period (low-pitch) autocorrelation lags.
  const trimThreshold = typeof o.trimThreshold === 'number' ? o.trimThreshold : 0;

  let mean = 0;
  for (let i = 0; i < SIZE; i++) mean += samples[i];
  mean /= SIZE;
  let rmsAcc = 0;
  for (let i = 0; i < SIZE; i++) {
    const x = samples[i] - mean;
    rmsAcc += x * x;
  }
  const rms = Math.sqrt(rmsAcc / SIZE);
  if (rms < rmsGate) return null;

  // DC-blocked working buffer.
  const work = new Float32Array(SIZE);
  for (let i = 0; i < SIZE; i++) work[i] = samples[i] - mean;

  let start = 0;
  let end = SIZE - 1;
  if (trimThreshold > 0) {
    for (let i = 0; i < SIZE; i++) {
      if (Math.abs(work[i]) >= trimThreshold) { start = i; break; }
    }
    for (let i = SIZE - 1; i >= 0; i--) {
      if (Math.abs(work[i]) >= trimThreshold) { end = i; break; }
    }
  }
  const trimmed = work.subarray(start, end + 1);
  const N = trimmed.length;
  if (N < 512) return null;

  const minLag = Math.floor(sampleRate / 1200);
  const maxLag = Math.min(N - 1, Math.floor(sampleRate / 60));

  // Signal energy = autocorrelation at lag 0. Used to normalize the
  // per-lag correlations so `clarity` ∈ [0, 1] regardless of signal
  // amplitude — a clean sine at any volume reaches clarity ≈ 1 at its
  // period, while noise stays well below.
  let energy = 0;
  for (let i = 0; i < N; i++) energy += trimmed[i] * trimmed[i];
  energy = energy / N;
  if (energy < 1e-10) return null;

  // Compute the autocorrelation across the lag range, normalized by
  // energy and window length. We then pick the FIRST local-maximum lag
  // whose normalized correlation exceeds the clarity threshold —
  // integer multiples of the fundamental period also produce strong
  // peaks but at higher lags (lower pitch), so the shortest valid lag
  // is the right answer.
  const corrs = new Float32Array(maxLag - minLag + 1);
  for (let lag = minLag; lag <= maxLag; lag++) {
    let c = 0;
    const e = N - lag;
    for (let i = 0; i < e; i++) c += trimmed[i] * trimmed[i + lag];
    corrs[lag - minLag] = (c / (e + 1e-9)) / energy;
  }

  let bestLag = -1;
  let bestCorr = 0;
  for (let i = 1; i < corrs.length - 1; i++) {
    const prev = corrs[i - 1];
    const cur = corrs[i];
    const next = corrs[i + 1];
    if (cur > minClarity && cur > prev && cur >= next) {
      bestLag = i + minLag;
      bestCorr = cur;
      break;
    }
  }
  // Fall back to global-max if no in-threshold local peak was found.
  // We still report the best-guess freq + its clarity so the caller can
  // decide whether to accept it (e.g. for a "what I'm hearing" preview
  // even when confidence is low).
  let belowThreshold = false;
  if (bestLag < 0) {
    for (let i = 0; i < corrs.length; i++) {
      if (corrs[i] > bestCorr) {
        bestCorr = corrs[i];
        bestLag = i + minLag;
      }
    }
    belowThreshold = true;
    if (bestLag < 0) return null;
  }

  // Parabolic interpolation around the integer peak, using neighbours
  // from the already-computed `corrs` table.
  let refined = bestLag;
  const idx = bestLag - minLag;
  if (idx > 0 && idx < corrs.length - 1) {
    const a = corrs[idx - 1];
    const b = bestCorr;
    const c = corrs[idx + 1];
    const denom = a - 2 * b + c;
    if (Math.abs(denom) > 1e-9) refined = bestLag + 0.5 * (a - c) / denom;
  }
  return {
    freq: sampleRate / refined,
    clarity: bestCorr,
    belowThreshold,
  };
}
