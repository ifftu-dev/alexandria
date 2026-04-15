// Music Instrument Trainer — canonical capability-prompt demo (Phase 1+3).
//
// Shows a target note, asks the host for `microphone`, and (on grant)
// taps an AudioContext to display a live amplitude meter. Pure UI — no
// grader, no submission, no host-mediated state. The "Done" button calls
// alex.complete(1, null) so the player marks the element complete.
//
// Real pitch detection (autocorrelation / YIN) is out of scope for this
// demo — the goal is to prove the capability flow end-to-end. Hooking
// in a real pitch tracker is a small, additive step on top of the audio
// stream we already capture here.

(function () {
  'use strict';

  const noteEl = document.getElementById('note');
  const meterFillEl = document.getElementById('meter-fill');
  const meterReadoutEl = document.getElementById('meter-readout');
  const grantBtn = document.getElementById('grant');
  const doneBtn = document.getElementById('done');
  const statusEl = document.getElementById('status');

  let audioCtx = null;
  let analyser = null;
  let stream = null;
  let rafId = null;

  alex.onHost((msg) => {
    if (msg.type === 'init') {
      const content = (msg.payload && msg.payload.content) || {};
      if (typeof content.note === 'string') noteEl.textContent = content.note;
    } else if (msg.type === 'capability_granted') {
      // The host has updated grants; if it's microphone, kick off audio.
      if (msg.payload && msg.payload.name === 'microphone') {
        startMic().catch((e) => {
          statusEl.textContent = `Microphone error: ${e.message || e}`;
        });
      }
    } else if (msg.type === 'capability_revoked') {
      if (msg.payload && msg.payload.name === 'microphone') {
        stopMic();
        meterReadoutEl.textContent = 'Microphone access revoked';
        grantBtn.disabled = false;
      }
    }
  });

  grantBtn.addEventListener('click', async () => {
    grantBtn.disabled = true;
    statusEl.textContent = '';
    try {
      const res = await alex.requestCapability(
        'microphone',
        'Listen for the note you play and show a live amplitude meter.',
      );
      const granted = !!(res && res.granted);
      if (!granted) {
        grantBtn.disabled = false;
        statusEl.textContent = 'Microphone access denied. You can still mark this complete.';
        doneBtn.disabled = false;
        return;
      }
      await startMic();
    } catch (e) {
      grantBtn.disabled = false;
      statusEl.textContent = `Could not request capability: ${e.message || e}`;
    }
  });

  doneBtn.addEventListener('click', () => {
    stopMic();
    void alex.complete(1, null);
    statusEl.textContent = 'Marked complete.';
    doneBtn.disabled = true;
  });

  async function startMic() {
    if (stream) return;
    try {
      stream = await navigator.mediaDevices.getUserMedia({
        audio: { echoCancellation: false, noiseSuppression: false, autoGainControl: false },
      });
    } catch (e) {
      throw new Error(e.message || String(e));
    }
    audioCtx = new (window.AudioContext || window.webkitAudioContext)();
    const src = audioCtx.createMediaStreamSource(stream);
    analyser = audioCtx.createAnalyser();
    analyser.fftSize = 1024;
    src.connect(analyser);
    meterReadoutEl.textContent = 'Listening…';
    doneBtn.disabled = false;
    tick();
  }

  function tick() {
    if (!analyser) return;
    const buf = new Float32Array(analyser.fftSize);
    analyser.getFloatTimeDomainData(buf);
    let sumSquares = 0;
    for (let i = 0; i < buf.length; i++) sumSquares += buf[i] * buf[i];
    const rms = Math.sqrt(sumSquares / buf.length);
    const pct = Math.min(100, Math.round(rms * 200 * 100));
    meterFillEl.style.width = pct + '%';
    meterReadoutEl.textContent = `RMS: ${(rms * 100).toFixed(1)}%`;
    rafId = requestAnimationFrame(tick);
  }

  function stopMic() {
    if (rafId) cancelAnimationFrame(rafId);
    rafId = null;
    if (analyser) {
      try { analyser.disconnect(); } catch (_) {}
      analyser = null;
    }
    if (audioCtx) {
      try { void audioCtx.close(); } catch (_) {}
      audioCtx = null;
    }
    if (stream) {
      for (const track of stream.getTracks()) track.stop();
      stream = null;
    }
    meterFillEl.style.width = '0%';
  }

  // Hand off to the host. We declare microphone up front so the manifest
  // and runtime declarations agree.
  void alex.ready(['microphone']);
})();
