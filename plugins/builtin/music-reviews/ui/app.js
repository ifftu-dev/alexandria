// Music Reviews — scrolling-timeline ear-training plugin.
//
// Notes flow right-to-left toward a vertical hit line. As each note crosses
// the line the plugin compares live mic pitch against the note's target
// frequency and marks it correct or wrong. Tempo-driven; pitch detection
// uses autocorrelation on the Web Audio API time-domain stream.

import { detectPitch, noteToMidi, freqToMidiFloat, midiToName } from './pitch.js';

(function () {
  'use strict';

  // ============================ CONFIG ===============================
  const DEFAULTS = {
    title: 'Warm-up scale',
    // Notes can be plain strings ("C4" = quarter note) or objects with an
    // explicit duration in beats: { name: "C4", duration: 2 }.
    notes: [
      { name: 'C4', duration: 1 },
      { name: 'D4', duration: 1 },
      { name: 'E4', duration: 1 },
      { name: 'F4', duration: 1 },
      { name: 'G4', duration: 2 },
      { name: 'A4', duration: 1 },
      { name: 'G4', duration: 2 },
      { name: 'F4', duration: 1 },
      { name: 'E4', duration: 2 },
      { name: 'D4', duration: 1 },
      { name: 'C4', duration: 4 },
    ],
    tolerance_cents: 45,
    bpm: 60,
    pxPerBeat: 220,
    // Fraction of a note's duration the learner must hold the correct pitch
    // for it to count as a hit. 0.5 = half the note, rounded down to at
    // least one frame at low durations.
    holdFraction: 0.45,
    // How early before a note's start the play head considers the note
    // "active" — gives the learner a beat of lead-in for the attack.
    leadInBeats: 0.5,
  };

  // ============================ STATE ================================
  const state = {
    title: DEFAULTS.title,
    notes: [],          // { id, name, midi, beat, duration, endBeat }
    bpm: DEFAULTS.bpm,
    pxPerBeat: DEFAULTS.pxPerBeat,
    tolerance: DEFAULTS.tolerance_cents,
    holdFraction: DEFAULTS.holdFraction,
    leadInBeats: DEFAULTS.leadInBeats,
    phase: 'idle',
    startTimeMs: 0,
    elapsedAtPauseSec: 0,
    cursor: 0,
    perNote: [],        // { name, beat, duration, status, centsOff, played, heldBeats }
    streak: 0,
    correct: 0,
    completed: 0,
    detectedPitch: null,
    lastSeenPitch: null,
    lastFrameSec: 0,
  };

  // ============================ DOM ==================================
  const $ = (id) => document.getElementById(id);
  const stage = $('stage');
  const notesLayer = $('notes');
  const ticksLayer = $('ticks');
  const statScore = $('stat-score');
  const statStreak = $('stat-streak');
  const statProgress = $('stat-progress');
  const titleEl = $('exercise-title');
  const bpmValue = $('bpm-value');
  const liveNote = $('live-note');
  const needle = $('needle');
  const vuFill = $('vu-fill');
  const vuMeter = $('vu-meter');
  const micState = $('mic-state');
  const startBtn = $('start-btn');
  const pauseBtn = $('pause-btn');
  const restartBtn = $('restart-btn');
  const finishBtn = $('finish-btn');
  const countinEl = $('countin');
  const countinNum = $('countin-num');
  const resultsEl = $('results');
  const toast = $('toast');

  // ============================ HELPERS ==============================
  // Note utilities + autocorrelation pitch detector live in pitch.js so
  // they can be unit-tested headlessly. See pitch.test.js.

  function showToast(msg) {
    toast.textContent = msg;
    toast.classList.add('show');
    clearTimeout(showToast._t);
    showToast._t = setTimeout(() => toast.classList.remove('show'), 1400);
  }

  function setPhase(phase) {
    state.phase = phase;
    startBtn.disabled = phase === 'playing' || phase === 'countin';
    pauseBtn.disabled = phase !== 'playing';
    restartBtn.disabled = phase === 'idle' || phase === 'countin';
    finishBtn.disabled = phase === 'idle' || phase === 'countin';
    pauseBtn.textContent = phase === 'paused' ? 'Resume' : 'Pause';
  }

  function updateStats() {
    statStreak.textContent = String(state.streak);
    statProgress.textContent = `${state.completed}/${state.notes.length}`;
    const denom = Math.max(1, state.completed);
    const acc = state.completed === 0 ? 0 : state.correct / denom;
    statScore.textContent = `${Math.round(acc * 100)}%`;
  }

  // ============================ NOTES =================================
  function normalizeNote(entry) {
    if (typeof entry === 'string') return { name: entry, duration: 1 };
    if (entry && typeof entry === 'object') {
      return {
        name: String(entry.name || entry.note || ''),
        duration: Number(entry.duration) > 0 ? Number(entry.duration) : 1,
      };
    }
    return null;
  }

  function buildNotes(notesArr) {
    let beat = 1; // 1-indexed so the first note has lead-in space
    state.notes = [];
    for (const raw of notesArr) {
      const n = normalizeNote(raw);
      if (!n || !n.name) continue;
      state.notes.push({
        id: state.notes.length,
        name: n.name,
        midi: noteToMidi(n.name),
        beat,
        duration: n.duration,
        endBeat: beat + n.duration,
      });
      beat += n.duration;
    }
    state.perNote = state.notes.map((n) => ({
      name: n.name,
      beat: n.beat,
      duration: n.duration,
      status: 'pending',
      centsOff: null,
      played: null,
      heldBeats: 0,
    }));
    renderNotes();
    renderTicks();
    updateStats();
  }

  function renderNotes() {
    notesLayer.innerHTML = '';
    state.notes.forEach((n) => {
      const el = document.createElement('div');
      el.className = 'note';
      el.dataset.id = String(n.id);
      el.innerHTML = `
        <div class="pill">
          <div class="progress"></div>
          <span class="label">${n.name}</span>
        </div>
        <div class="meta">
          <span class="played" hidden></span>
          <span class="cents"></span>
        </div>`;
      notesLayer.appendChild(el);
    });
  }

  function renderTicks() {
    ticksLayer.innerHTML = '';
    const total = (state.notes.length > 0
      ? state.notes[state.notes.length - 1].endBeat
      : 0) + 2;
    for (let b = 0; b <= total; b++) {
      const t = document.createElement('div');
      t.className = 'tick ' + (b % 4 === 0 ? 'big' : 'small');
      t.dataset.beat = String(b);
      ticksLayer.appendChild(t);
    }
  }

  // hit line x in CSS pixels — read once per frame
  function hitXPx() {
    const r = stage.getBoundingClientRect();
    const hitVar = getComputedStyle(document.documentElement).getPropertyValue('--hit-x').trim();
    const pct = parseFloat(hitVar) || 32;
    return r.width * (pct / 100);
  }

  function noteXAt(beatOffset) {
    // Returns the CSS-pixel x position of a note whose beat number is
    // `noteBeat`, given that `elapsedBeats` beats have passed.
    return hitXPx() + beatOffset * state.pxPerBeat;
  }

  function elapsedSec() {
    if (state.phase === 'playing') {
      return state.elapsedAtPauseSec + (performance.now() - state.startTimeMs) / 1000;
    }
    return state.elapsedAtPauseSec;
  }

  function elapsedBeats() {
    return elapsedSec() * (state.bpm / 60);
  }

  // ============================ PITCH =================================
  let audioCtx = null;
  let analyser = null;
  let stream = null;
  let buffer = null;

  async function startMic() {
    if (stream) return;
    stream = await navigator.mediaDevices.getUserMedia({
      audio: { echoCancellation: false, noiseSuppression: false, autoGainControl: false },
    });
    audioCtx = new (window.AudioContext || window.webkitAudioContext)();
    const src = audioCtx.createMediaStreamSource(stream);
    analyser = audioCtx.createAnalyser();
    analyser.fftSize = 4096;
    analyser.smoothingTimeConstant = 0;
    buffer = new Float32Array(analyser.fftSize);
    src.connect(analyser);
  }

  function stopMic() {
    if (analyser) { try { analyser.disconnect(); } catch (_) {} analyser = null; }
    if (audioCtx) { try { void audioCtx.close(); } catch (_) {} audioCtx = null; }
    if (stream) { for (const t of stream.getTracks()) t.stop(); stream = null; }
  }

  // Clarity gate for accepting a detected pitch as "what the user is
  // playing". Below this, we still SHOW the pitch in the UI (so the
  // learner sees what the detector heard), but evaluation refuses it.
  const ACCEPT_CLARITY = 0.78;

  function readPitch() {
    if (!analyser) {
      renderVU(0, false, null);
      return null;
    }
    analyser.getFloatTimeDomainData(buffer);
    let rms = 0;
    for (let i = 0; i < buffer.length; i++) rms += buffer[i] * buffer[i];
    rms = Math.sqrt(rms / buffer.length);

    const det = detectPitch(buffer, audioCtx.sampleRate, { minClarity: ACCEPT_CLARITY });
    renderVU(rms, true, det);

    // Always surface the best-guess pitch in the "You" display, even at
    // low clarity — gives the learner visibility into what the detector
    // is hearing. The needle/evaluation gating happens upstream.
    if (det) {
      const midiFloat = freqToMidiFloat(det.freq);
      state.lastSeenPitch = { midi: midiFloat, name: midiToName(midiFloat), clarity: det.clarity };
    } else {
      state.lastSeenPitch = null;
    }

    if (!det || det.belowThreshold) return null;
    const midiFloat = freqToMidiFloat(det.freq);
    return { midi: midiFloat, name: midiToName(midiFloat) };
  }

  function renderVU(rms, live, det) {
    const db = rms > 0 ? 20 * Math.log10(rms) : -100;
    const pct = Math.max(0, Math.min(100, ((db + 60) / 60) * 100));
    if (vuFill) vuFill.style.width = `${pct}%`;
    if (vuMeter) vuMeter.classList.toggle('silent', live && rms < 0.003);
    if (micState) {
      if (!live) {
        micState.textContent = 'off';
      } else if (rms < 0.003) {
        micState.textContent = 'silent';
      } else if (!det) {
        micState.textContent = 'no pitch';
      } else if (det.belowThreshold) {
        micState.textContent = `weak (clarity ${(det.clarity * 100).toFixed(0)}%)`;
      } else {
        micState.textContent = `locked (clarity ${(det.clarity * 100).toFixed(0)}%)`;
      }
    }
  }

  // ============================ MAIN LOOP =============================
  let rafId = null;
  function tick() {
    rafId = requestAnimationFrame(tick);

    const pitch = readPitch();
    state.detectedPitch = pitch;
    renderPitchDisplay(pitch);

    const eBeats = elapsedBeats();
    const eSec = elapsedSec();
    const dtSec = Math.max(0, eSec - state.lastFrameSec);
    state.lastFrameSec = eSec;
    const dtBeats = dtSec * (state.bpm / 60);

    // 1) Position + style every note tile based on the play head.
    const els = notesLayer.children;
    state.notes.forEach((n, i) => {
      const el = els[i];
      if (!el) return;
      const startOffset = n.beat - eBeats;       // beats until note start
      const endOffset = n.endBeat - eBeats;      // beats until note end
      const xLeft = noteXAt(startOffset);
      const widthPx = n.duration * state.pxPerBeat;
      el.style.left = `${xLeft}px`;
      el.style.width = `${widthPx}px`;
      const pill = el.querySelector('.pill');
      if (pill) pill.style.width = `${widthPx}px`;

      const pn = state.perNote[i];
      const isActive = pn.status === 'pending' && startOffset <= state.leadInBeats && endOffset >= -0.1;
      el.classList.toggle('active', isActive);
      el.classList.toggle('ok', pn.status === 'ok');
      el.classList.toggle('bad', pn.status === 'bad');

      // Progress fill — fraction of the note's duration the play head has
      // covered (when active) OR fraction held correctly (post-evaluation).
      const progressEl = el.querySelector('.progress');
      if (progressEl) {
        let frac = 0;
        if (pn.status === 'ok' || pn.status === 'bad') {
          frac = Math.min(1, pn.heldBeats / Math.max(0.001, n.duration));
        } else if (isActive) {
          const traversedBeats = Math.min(n.duration, Math.max(0, eBeats - n.beat));
          frac = traversedBeats / Math.max(0.001, n.duration);
        }
        progressEl.style.width = `${Math.round(frac * 100)}%`;
      }

      // Cull off-screen far past.
      const stageW = stage.getBoundingClientRect().width;
      if (xLeft + widthPx < -50 || xLeft > stageW + 50) {
        el.style.display = 'none';
      } else {
        el.style.display = '';
      }
    });

    // 2) Beat ticks.
    const tickEls = ticksLayer.children;
    const stageW = stage.getBoundingClientRect().width;
    for (let i = 0; i < tickEls.length; i++) {
      const beat = parseFloat(tickEls[i].dataset.beat);
      const x = noteXAt(beat - eBeats);
      if (x < -4 || x > stageW + 4) {
        tickEls[i].style.display = 'none';
      } else {
        tickEls[i].style.display = '';
        tickEls[i].style.left = `${x}px`;
      }
    }

    // 3) Accumulate held-time + evaluate.
    if (state.phase === 'playing') {
      evaluateCursor(eBeats, pitch, dtBeats);
    }
  }

  function evaluateCursor(eBeats, pitch, dtBeats) {
    while (state.cursor < state.notes.length) {
      const n = state.notes[state.cursor];
      const pn = state.perNote[state.cursor];
      const startOffset = n.beat - eBeats;
      const endOffset = n.endBeat - eBeats;

      // Note hasn't reached the hit line yet.
      if (startOffset > state.leadInBeats) break;

      if (pn.status !== 'pending') {
        state.cursor++;
        continue;
      }

      // Accumulate hold time while the learner is on-pitch and the note
      // is at-or-past the hit line.
      if (startOffset <= 0 && endOffset > -0.05 && pitch && n.midi !== null) {
        const cents = (pitch.midi - n.midi) * 100;
        if (Math.abs(cents) <= state.tolerance) {
          pn.heldBeats += dtBeats;
          pn.centsOff = cents;
          if (!pn.played) pn.played = pitch.name;
        }
      }

      const required = Math.max(0.05, n.duration * state.holdFraction);

      // Early hit — held long enough before the note ends.
      if (pn.heldBeats >= required) {
        markNote(state.cursor, 'ok', pn.centsOff);
        state.cursor++;
        continue;
      }

      // Note ended without enough sustained hold.
      if (endOffset < -0.05) {
        const cents = pitch && n.midi !== null ? (pitch.midi - n.midi) * 100 : pn.centsOff;
        markNote(state.cursor, 'bad', cents);
        state.cursor++;
        continue;
      }

      break;
    }

    if (state.completed >= state.notes.length && state.phase !== 'finished') {
      finishExercise();
    }
  }

  function markNote(i, status, cents) {
    const pn = state.perNote[i];
    pn.status = status;
    if (cents !== null && cents !== undefined) pn.centsOff = cents;
    // If the learner never held a matching pitch during the note,
    // snapshot whatever pitch the mic last detected so the tile shows
    // "you played: F#" instead of nothing.
    if (!pn.played) {
      const pitch = state.detectedPitch;
      pn.played = pitch ? pitch.name : null;
    }
    state.completed++;
    if (status === 'ok') {
      state.correct++;
      state.streak++;
      void alex.emitEvent('note_correct', {
        target: state.notes[i].name,
        played: state.perNote[i].played,
        centsOff: cents,
      });
    } else {
      state.streak = 0;
      void alex.emitEvent('note_wrong', {
        target: state.notes[i].name,
        played: state.perNote[i].played,
        centsOff: cents,
      });
    }
    updateStats();
    const el = notesLayer.children[i];
    if (el) {
      const playedEl = el.querySelector('.played');
      if (playedEl) {
        if (pn.played) {
          playedEl.textContent = pn.played;
          playedEl.classList.toggle('ok', status === 'ok');
          playedEl.classList.toggle('bad', status === 'bad');
          playedEl.hidden = false;
        } else {
          playedEl.hidden = true;
        }
      }
      const centsEl = el.querySelector('.cents');
      if (centsEl) {
        const c = pn.centsOff;
        if (c !== null && c !== undefined && !Number.isNaN(c)) {
          const sign = c > 0 ? '+' : '';
          centsEl.textContent = `${sign}${c.toFixed(0)}¢`;
        } else {
          centsEl.textContent = status === 'bad' ? 'missed' : '';
        }
      }
    }
  }

  function renderPitchDisplay(pitch) {
    // Prefer the locked pitch; fall back to the best-guess (low-clarity)
    // pitch so the learner always sees what the detector is hearing.
    const display = pitch || state.lastSeenPitch;
    if (!display) {
      liveNote.textContent = '—';
      liveNote.classList.add('dim');
      needle.style.left = '50%';
      needle.style.background = 'var(--accent)';
      return;
    }
    liveNote.textContent = display.name;
    liveNote.classList.toggle('dim', !pitch);
    // Needle position relative to currently-active target note (if any),
    // else to the nearest semitone.
    let cents = 0;
    let ref = null;
    if (state.cursor < state.notes.length) {
      ref = state.notes[state.cursor];
    }
    if (ref && ref.midi !== null) {
      cents = (display.midi - ref.midi) * 100;
    } else {
      cents = (display.midi - Math.round(display.midi)) * 100;
    }
    const clamped = Math.max(-50, Math.min(50, cents));
    const pct = 50 + clamped;
    needle.style.left = `${pct}%`;
    needle.style.background =
      Math.abs(cents) <= state.tolerance ? 'var(--ok)' :
      Math.abs(cents) <= state.tolerance * 2 ? 'var(--warn)' : 'var(--bad)';
  }

  // ============================ FLOW =================================
  async function play() {
    if (state.phase === 'paused') {
      state.startTimeMs = performance.now();
      setPhase('playing');
      return;
    }
    try {
      const res = await alex.requestCapability(
        'microphone',
        'Live pitch detection compares what you play against the target notes — audio never leaves your device.',
      );
      if (!res || !res.granted) {
        showToast('Microphone access denied');
        return;
      }
      await startMic();
    } catch (e) {
      showToast(`Mic error: ${e.message || e}`);
      return;
    }
    // Count-in for 3 beats so the first note doesn't surprise the user.
    setPhase('countin');
    await runCountIn(3);
    state.elapsedAtPauseSec = 0;
    state.startTimeMs = performance.now();
    state.lastFrameSec = 0;
    setPhase('playing');
  }

  function runCountIn(beats) {
    return new Promise((resolve) => {
      countinEl.hidden = false;
      let n = beats;
      const intervalMs = (60 / state.bpm) * 1000;
      countinNum.textContent = String(n);
      const step = () => {
        n--;
        if (n <= 0) {
          countinEl.hidden = true;
          resolve();
          return;
        }
        countinNum.textContent = String(n);
        countinNum.style.animation = 'none';
        // restart animation
        void countinNum.offsetWidth;
        countinNum.style.animation = '';
        setTimeout(step, intervalMs);
      };
      setTimeout(step, intervalMs);
    });
  }

  function pause() {
    if (state.phase === 'playing') {
      state.elapsedAtPauseSec = elapsedSec();
      setPhase('paused');
    } else if (state.phase === 'paused') {
      state.startTimeMs = performance.now();
      setPhase('playing');
    }
  }

  function restart() {
    state.cursor = 0;
    state.streak = 0;
    state.correct = 0;
    state.completed = 0;
    state.elapsedAtPauseSec = 0;
    state.perNote = state.notes.map((n) => ({ name: n.name, beat: n.beat, status: 'pending', centsOff: null }));
    resultsEl.hidden = true;
    updateStats();
    setPhase('idle');
    void play();
  }

  function finishExercise() {
    const denom = Math.max(1, state.notes.length);
    const score = state.correct / denom;
    renderResults(score);
    stopMic();
    setPhase('finished');
    void alex.complete(1, score);
  }

  function finishEarly() {
    // Mark any unevaluated notes wrong + finish.
    for (let i = state.cursor; i < state.notes.length; i++) {
      if (state.perNote[i].status === 'pending') {
        markNote(i, 'bad', null);
      }
    }
    finishExercise();
  }

  function renderResults(score) {
    resultsEl.hidden = false;
    resultsEl.innerHTML = '';
    const card = document.createElement('div');
    card.className = 'card';
    const h = document.createElement('h2');
    h.textContent = 'Run complete';
    const big = document.createElement('div');
    big.className = 'score';
    big.textContent = `${Math.round(score * 100)}%`;
    const sub = document.createElement('div');
    sub.className = 'sub';
    sub.textContent = `${state.correct} of ${state.notes.length} notes hit · best streak ${state.streak}`;
    const grid = document.createElement('div');
    grid.className = 'grid';
    state.perNote.forEach((p) => {
      const c = document.createElement('div');
      c.className = `cell ${p.status === 'ok' ? 'ok' : 'bad'}`;
      const cents = typeof p.centsOff === 'number'
        ? ` ${p.centsOff > 0 ? '+' : ''}${p.centsOff.toFixed(0)}¢`
        : '';
      const playedLine = p.played && p.played !== p.name ? `\nyou: ${p.played}` : '';
      c.innerHTML = `<div>${p.name}${cents}</div>${
        playedLine ? `<div style="font-size:10px;opacity:0.75;font-weight:400;">${playedLine.trim()}</div>` : ''
      }`;
      grid.appendChild(c);
    });
    const actions = document.createElement('div');
    actions.className = 'actions';
    const again = document.createElement('button');
    again.textContent = 'Try again';
    again.addEventListener('click', () => {
      resultsEl.hidden = true;
      restart();
    });
    actions.appendChild(again);

    card.appendChild(h);
    card.appendChild(big);
    card.appendChild(sub);
    card.appendChild(grid);
    card.appendChild(actions);
    resultsEl.appendChild(card);
  }

  // ============================ BUTTONS ===============================
  startBtn.addEventListener('click', () => void play());
  pauseBtn.addEventListener('click', () => pause());
  restartBtn.addEventListener('click', () => restart());
  finishBtn.addEventListener('click', () => finishEarly());
  $('bpm-up').addEventListener('click', () => setBpm(state.bpm + 5));
  $('bpm-down').addEventListener('click', () => setBpm(state.bpm - 5));

  function setBpm(v) {
    state.bpm = Math.max(40, Math.min(180, v));
    bpmValue.textContent = String(state.bpm);
  }

  // ============================ HOST WIRING ===========================
  alex.onHost((msg) => {
    if (msg.type === 'init') {
      const content = (msg.payload && msg.payload.content) || {};
      if (typeof content.title === 'string') {
        state.title = content.title;
        titleEl.textContent = content.title;
      }
      if (typeof content.tolerance_cents === 'number') state.tolerance = content.tolerance_cents;
      if (typeof content.bpm === 'number') setBpm(content.bpm);
      const list = Array.isArray(content.notes) && content.notes.length > 0
        ? content.notes
        : DEFAULTS.notes;
      buildNotes(list);
    } else if (msg.type === 'capability_revoked' && msg.payload && msg.payload.name === 'microphone') {
      stopMic();
      setPhase('idle');
      showToast('Mic access revoked');
    }
  });

  // Initial render with defaults (host init replaces if it arrives later).
  titleEl.textContent = state.title;
  bpmValue.textContent = String(state.bpm);
  buildNotes(DEFAULTS.notes);
  setPhase('idle');
  rafId = requestAnimationFrame(tick);

  void alex.ready(['microphone']);
})();
