// Language Lab — one element = one activity, chosen by `content.activity`:
// dictation | reading | writing | builder | read_aloud.
//
// Graded activities submit through `alex.submit`; the host runs the
// deterministic language-lab grader and replies with the score. Answer keys
// live in `content.grader_private`, which the host strips before `init`, so
// this script never sees them. Read-aloud recordings go to the instructor
// inbox (`type: 'irl_review'`) and can be checked by the host's on-device
// speech model (`speech_check` event).

import {
  bytesToBase64,
  extForMime,
  floatToPcm16LE,
  glossarySegments,
  pickVoice,
  readSettings,
  tokens,
  wordDiff,
  writingTips,
} from './lang.js';
import { str } from './i18n.js';

const STATE_VERSION = 1;
const MAX_RECORD_MS = 30_000;
const SPEECH_SAMPLE_RATE = 16_000;

const root = document.getElementById('app');

let content = {};
let settings = readSettings(null);
let lang = settings.ui_language;
let granted = new Set();
let state = freshState('');
/** Live, non-persisted recordings: index → { blob, url, mime }. */
const recordings = new Map();
/** Non-persisted UI flags: index → { busy, error, packMissing, unavailable }. */
const itemUi = new Map();
let activeRecorder = null; // { index, recorder, stream, timer, tick, startedAt }
let footerStatus = null; // { kind, text }
let submitting = false;

// ------------------------------------------------------------------ helpers

function freshState(activity) {
  return {
    v: STATE_VERSION,
    activity,
    lang: null,
    answers: [],
    selected: [],
    orders: [],
    score: null,
    submitted: false,
    recorded: [],
    sent: [],
    checks: [],
    completed: false,
  };
}

function restoreState(raw, activity) {
  if (!raw || typeof raw !== 'object' || raw.v !== STATE_VERSION || raw.activity !== activity) {
    return freshState(activity);
  }
  const s = { ...freshState(activity), ...raw };
  for (const k of ['answers', 'selected', 'orders', 'recorded', 'sent', 'checks']) {
    if (!Array.isArray(s[k])) s[k] = [];
  }
  return s;
}

let persistTimer = null;
function persist() {
  clearTimeout(persistTimer);
  persistTimer = setTimeout(() => {
    void alex.persistState(state).catch(() => {});
  }, 350);
}

/** Tiny DOM builder: h('div', { class: 'x', onclick: fn }, child, 'text'). */
function h(tag, attrs, ...children) {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs || {})) {
    if (v === null || v === undefined || v === false) continue;
    if (k.startsWith('on') && typeof v === 'function') el.addEventListener(k.slice(2), v);
    else if (k === 'class') el.className = v;
    else if (k === 'value') el.value = String(v);
    else if (k in el && typeof v !== 'string') el[k] = v;
    else el.setAttribute(k, v === true ? '' : String(v));
  }
  for (const c of children.flat()) {
    if (c === null || c === undefined || c === false) continue;
    el.append(c instanceof Node ? c : document.createTextNode(String(c)));
  }
  return el;
}

/** Bilingual text node: Hindi first with English beneath, or English only. */
function bi(key, params) {
  if (lang === 'en') return h('span', { lang: 'en' }, str(key, 'en', params));
  return h(
    'span',
    { class: 'bi' },
    h('span', { class: 'pri', lang: 'hi' }, str(key, 'hi', params)),
    h('span', { class: 'sec', lang: 'en' }, str(key, 'en', params)),
  );
}

/** Plain string in the current language (for aria-labels / placeholders). */
function t(key, params) {
  return str(key, lang, params);
}

function errText(e) {
  return String((e && e.message) || e || 'unknown error');
}

function items() {
  return Array.isArray(content.items) ? content.items : [];
}

function hintEl(text) {
  if (!settings.show_hints || typeof text !== 'string' || !text.trim()) return null;
  return h('div', { class: 'hint', lang: 'hi' }, h('b', {}, t('hint') + ': '), text);
}

// --------------------------------------------------------------------- TTS

const tts = {
  supported:
    typeof window.speechSynthesis !== 'undefined' &&
    typeof window.SpeechSynthesisUtterance !== 'undefined',
  voice: null,
  noEnglish: false,
  load() {
    if (!this.supported) return;
    const pick = () => {
      const voices = window.speechSynthesis.getVoices() || [];
      const before = this.noEnglish;
      this.voice = pickVoice(voices);
      this.noEnglish = voices.length > 0 && !this.voice;
      if (before !== this.noEnglish && content.activity) render();
    };
    pick();
    if (typeof window.speechSynthesis.addEventListener === 'function') {
      window.speechSynthesis.addEventListener('voiceschanged', pick);
    } else {
      window.speechSynthesis.onvoiceschanged = pick;
    }
  },
  speak(text, slow) {
    if (!this.supported || !text) return;
    const synth = window.speechSynthesis;
    synth.cancel();
    const u = new SpeechSynthesisUtterance(text);
    const rate = settings.speech_rate * (slow ? 0.65 : 1);
    u.rate = Math.max(0.3, Math.min(2, rate));
    if (this.voice) {
      u.voice = this.voice;
      u.lang = this.voice.lang;
    } else {
      u.lang = 'en-IN';
    }
    synth.speak(u);
  },
  stop() {
    if (this.supported) window.speechSynthesis.cancel();
  },
};

function listenButtons(text) {
  return [
    h(
      'button',
      { type: 'button', disabled: !tts.supported, onclick: () => tts.speak(text, false) },
      '▶ ',
      bi('listen'),
    ),
    h(
      'button',
      { type: 'button', disabled: !tts.supported, onclick: () => tts.speak(text, true) },
      '🐢 ',
      bi('listen_slow'),
    ),
  ];
}

// --------------------------------------------------------------- rendering

function render() {
  document.documentElement.lang = lang === 'en' ? 'en' : 'hi';
  const activity = content.activity;
  const renderer = RENDERERS[activity];
  root.replaceChildren();
  if (!renderer) {
    root.append(h('div', { class: 'notice bad' }, bi('unknown_activity')));
    return;
  }
  root.append(header(`${activity}_title`, `${activity}_intro`));
  if (!tts.supported) root.append(h('div', { class: 'notice warn' }, bi('tts_unavailable')));
  else if (tts.noEnglish) root.append(h('div', { class: 'notice' }, bi('no_english_voice')));
  renderer();
}

function header(titleKey, introKey) {
  const toggle = h(
    'button',
    {
      type: 'button',
      class: 'lang-toggle',
      lang: lang === 'en' ? 'hi' : 'en',
      onclick: () => {
        lang = lang === 'en' ? 'hi' : 'en';
        state.lang = lang;
        persist();
        render();
      },
    },
    str('switch_language', lang),
  );
  return h(
    'div',
    {},
    h('div', { class: 'head' }, h('h1', {}, bi(titleKey)), toggle),
    h('p', { class: 'intro' }, bi(introKey)),
  );
}

/** Submit footer shared by the graded activities. */
function gradedFooter(buildSubmission, hasAnswer) {
  const status = h('span', { class: 'status', role: 'status' });
  const button = h('button', { type: 'button', class: 'primary' }, bi('submit'));
  const footer = h('div', { class: 'footer' }, button, status);

  if (state.submitted) {
    button.replaceChildren(bi('try_again'));
    button.classList.remove('primary');
    button.addEventListener('click', () => {
      state.submitted = false;
      state.score = null;
      footerStatus = null;
      persist();
      render();
    });
    if (typeof state.score === 'number') {
      status.append(
        h('span', { class: 'score-badge' }, bi('score', { pct: Math.round(state.score * 100) })),
      );
    } else {
      status.append(bi('submitted_no_score'));
    }
    return footer;
  }

  if (footerStatus) {
    status.className = `status ${footerStatus.kind === 'error' ? 'error' : ''}`;
    status.append(footerStatus.node);
  }
  button.disabled = submitting;
  button.addEventListener('click', async () => {
    if (submitting) return;
    if (!hasAnswer()) {
      footerStatus = { kind: 'error', node: bi('need_answer') };
      render();
      return;
    }
    submitting = true;
    footerStatus = { kind: 'info', node: bi('submitting') };
    render();
    try {
      const resp = await alex.submit(buildSubmission(), { activity: content.activity });
      submitting = false;
      footerStatus = null;
      onScored(resp && typeof resp.score === 'number' ? resp.score : null);
    } catch (e) {
      submitting = false;
      footerStatus = { kind: 'error', node: bi('submit_failed', { error: errText(e) }) };
      render();
    }
  });
  return footer;
}

function onScored(score) {
  state.submitted = true;
  state.score = score;
  persist();
  void alex.complete(1, typeof score === 'number' ? score : undefined).catch(() => {});
  render();
}

// --------------------------------------------------------------- dictation

function renderDictation() {
  const list = h('div', { class: 'items' });
  items().forEach((item, i) => {
    const text = String(item?.text ?? '');
    const input = h('input', {
      type: 'text',
      value: state.answers[i] ?? '',
      placeholder: t('type_here'),
      autocomplete: 'off',
      autocapitalize: 'off',
      spellcheck: 'false',
      disabled: state.submitted,
      'aria-label': t('item_n', { n: i + 1 }),
      oninput: (e) => {
        state.answers[i] = e.target.value;
        persist();
      },
    });
    const card = h(
      'section',
      { class: 'card' },
      h('div', { class: 'card-label' }, bi('item_n', { n: i + 1 })),
      h('div', { class: 'row' }, ...listenButtons(text)),
      h('div', { class: 'field row' }, input),
      hintEl(item?.hint_hi),
    );
    if (state.submitted) {
      const { ops } = wordDiff(tokens(text), tokens(state.answers[i] ?? ''));
      card.append(
        h(
          'div',
          { class: 'words', 'aria-label': t('correct_sentence') },
          ops.map((op) => h('span', { class: `w ${op.status}` }, op.word)),
        ),
        h('div', { class: 'hint' }, h('b', {}, t('correct_sentence') + ': '), h('span', { lang: 'en' }, text)),
      );
    }
    list.append(card);
  });
  root.append(list);
  if (state.submitted) root.append(h('div', { class: 'legend' }, bi('diff_legend')));
  root.append(
    gradedFooter(
      () => ({ answers: items().map((_, i) => String(state.answers[i] ?? '')) }),
      () => items().some((_, i) => String(state.answers[i] ?? '').trim() !== ''),
    ),
  );
}

// ----------------------------------------------------------------- reading

let openGloss = null; // { button, popover }

function closeGloss() {
  if (!openGloss) return;
  openGloss.button.setAttribute('aria-expanded', 'false');
  openGloss.popover.remove();
  openGloss = null;
}

function showGloss(button, wrap, seg) {
  const same = openGloss && openGloss.button === button;
  closeGloss();
  if (same) return;
  const popover = h(
    'div',
    { class: 'popover', role: 'status' },
    h('div', { class: 'pw', lang: 'en' }, seg.text),
    h('div', { class: 'pm', lang: 'hi' }, seg.meaning),
  );
  wrap.append(popover);
  const bw = button.offsetWidth;
  const left = Math.max(0, Math.min(button.offsetLeft + bw / 2 - popover.offsetWidth / 2, wrap.clientWidth - popover.offsetWidth));
  popover.style.left = `${left}px`;
  popover.style.top = `${button.offsetTop + button.offsetHeight + 6}px`;
  button.setAttribute('aria-expanded', 'true');
  openGloss = { button, popover };
}

document.addEventListener('click', (e) => {
  if (openGloss && !openGloss.button.contains(e.target) && !openGloss.popover.contains(e.target)) closeGloss();
});
document.addEventListener('keydown', (e) => {
  if (e.key === 'Escape') closeGloss();
});

function renderReading() {
  openGloss = null;
  const passage = String(content.passage ?? '');
  const wrap = h('div', { class: 'passage-wrap' });
  const p = h('p', { class: 'passage', lang: 'en' });
  const segments = settings.show_hints ? glossarySegments(passage, content.glossary) : [{ text: passage }];
  for (const seg of segments) {
    if (!seg.meaning) {
      p.append(seg.text);
      continue;
    }
    const b = h('button', {
      type: 'button',
      class: 'gloss',
      'aria-expanded': 'false',
      'aria-label': `${seg.text}: ${t('meaning')}`,
    }, seg.text);
    b.addEventListener('click', (e) => {
      e.stopPropagation();
      showGloss(b, wrap, seg);
    });
    p.append(b);
  }
  wrap.append(p);
  root.append(
    h(
      'section',
      { class: 'card' },
      h('div', { class: 'row' }, h('button', {
        type: 'button',
        disabled: !tts.supported,
        onclick: () => tts.speak(passage, false),
      }, '▶ ', bi('read_passage')), h('button', {
        type: 'button',
        disabled: !tts.supported,
        onclick: () => tts.stop(),
      }, '■ ', bi('stop_audio'))),
      h('div', { class: 'row' }, wrap),
    ),
  );

  const questions = Array.isArray(content.questions) ? content.questions : [];
  const list = h('div', { class: 'items', style: 'margin-top:12px' });
  questions.forEach((q, qi) => {
    const options = Array.isArray(q?.options) ? q.options : [];
    const legendChildren = [
      h('div', { class: 'card-label' }, bi('question_n', { n: qi + 1 })),
      h('div', { lang: 'en' }, String(q?.prompt ?? '')),
    ];
    if (settings.show_hints && q?.prompt_hi) {
      legendChildren.push(h('div', { class: 'hint', lang: 'hi' }, q.prompt_hi));
    }
    const fs = h('fieldset', { disabled: state.submitted }, h('legend', {}, ...legendChildren));
    options.forEach((opt, oi) => {
      const checked = state.selected[qi] === oi;
      const input = h('input', {
        type: 'radio',
        name: `q${qi}`,
        value: String(oi),
        checked,
        onchange: () => {
          state.selected[qi] = oi;
          persist();
          for (const lab of fs.querySelectorAll('.option')) {
            lab.classList.toggle('selected', lab.querySelector('input')?.checked === true);
          }
        },
      });
      fs.append(h('label', { class: `option${checked ? ' selected' : ''}` }, input, h('span', { lang: 'en' }, String(opt))));
    });
    list.append(h('section', { class: 'card' }, fs));
  });
  root.append(list);
  root.append(
    gradedFooter(
      () => ({ selected: questions.map((_, i) => (Number.isInteger(state.selected[i]) ? state.selected[i] : null)) }),
      () => questions.some((_, i) => Number.isInteger(state.selected[i])),
    ),
  );
}

// ----------------------------------------------------------------- writing

function tipNodes(text) {
  const tips = writingTips(text);
  if (tips.length === 0) {
    return tokens(text).length ? [h('li', { class: 'good' }, bi('looks_good'))] : [];
  }
  return tips.map((tip) =>
    h('li', {}, bi(`tip_${tip.kind}`, { next: tip.next, expected: tip.expected, article: tip.article })),
  );
}

function renderWriting() {
  const list = h('div', { class: 'items' });
  items().forEach((item, i) => {
    const tipsEl = h('ul', { class: 'tips', 'aria-live': 'polite' }, tipNodes(state.answers[i] ?? ''));
    let tipTimer = null;
    const area = h('textarea', {
      rows: 2,
      value: state.answers[i] ?? '',
      placeholder: t('type_here'),
      autocapitalize: 'off',
      spellcheck: 'false',
      disabled: state.submitted,
      'aria-label': item?.prompt_en || item?.prompt_hi || t('item_n', { n: i + 1 }),
      oninput: (e) => {
        state.answers[i] = e.target.value;
        persist();
        clearTimeout(tipTimer);
        tipTimer = setTimeout(() => tipsEl.replaceChildren(...tipNodes(state.answers[i] ?? '')), 300);
      },
    });
    const prompt = h('div', { class: 'say', lang: 'hi' }, String(item?.prompt_hi ?? ''));
    const promptEn =
      lang === 'en' && item?.prompt_en ? h('div', { class: 'hint', lang: 'en' }, item.prompt_en) : null;
    list.append(
      h(
        'section',
        { class: 'card' },
        h('div', { class: 'card-label' }, bi('item_n', { n: i + 1 })),
        prompt,
        promptEn,
        h('div', { class: 'field row' }, area),
        tipsEl,
        hintEl(item?.hint_hi),
      ),
    );
  });
  root.append(list);
  root.append(
    gradedFooter(
      () => ({ answers: items().map((_, i) => String(state.answers[i] ?? '')) }),
      () => items().some((_, i) => String(state.answers[i] ?? '').trim() !== ''),
    ),
  );
}

// ----------------------------------------------------------------- builder

function renderBuilder() {
  const list = h('div', { class: 'items' });
  items().forEach((item, i) => {
    const tiles = Array.isArray(item?.tiles) ? item.tiles.map(String) : [];
    const order = (Array.isArray(state.orders[i]) ? state.orders[i] : []).filter(
      (x) => Number.isInteger(x) && x >= 0 && x < tiles.length,
    );
    state.orders[i] = order;
    const locked = state.submitted;
    const update = (next) => {
      state.orders[i] = next;
      persist();
      render();
    };
    const built = h('div', { class: 'built', 'aria-label': t('builder_empty') });
    if (order.length === 0) built.append(h('span', { class: 'placeholder' }, bi('builder_empty')));
    order.forEach((ti, pos) => {
      built.append(
        h('button', {
          type: 'button',
          class: 'tile',
          lang: 'en',
          disabled: locked,
          onclick: () => update(order.filter((_, p) => p !== pos)),
        }, tiles[ti]),
      );
    });
    const bank = h('div', { class: 'bank' });
    tiles.forEach((word, ti) => {
      const used = order.includes(ti);
      bank.append(
        h('button', {
          type: 'button',
          class: `tile${used ? ' used' : ''}`,
          lang: 'en',
          disabled: locked || used,
          'aria-hidden': used ? 'true' : null,
          onclick: () => update([...order, ti]),
        }, word),
      );
    });
    list.append(
      h(
        'section',
        { class: 'card' },
        h('div', { class: 'card-label' }, bi('item_n', { n: i + 1 })),
        hintEl(item?.hint_hi),
        h('div', { class: 'row' }, built),
        bank,
        h('div', { class: 'row' }, h('button', {
          type: 'button',
          disabled: locked || order.length === 0,
          onclick: () => update([]),
        }, '↺ ', bi('reset'))),
      ),
    );
  });
  root.append(list);
  root.append(
    gradedFooter(
      () => ({ orders: items().map((_, i) => (Array.isArray(state.orders[i]) ? state.orders[i] : [])) }),
      () => items().some((_, i) => Array.isArray(state.orders[i]) && state.orders[i].length > 0),
    ),
  );
}

// -------------------------------------------------------------- read aloud

function ui(i) {
  if (!itemUi.has(i)) itemUi.set(i, {});
  return itemUi.get(i);
}

async function ensureCapability(name, reasonKey) {
  if (granted.has(name)) return true;
  const reason = `${str(reasonKey, 'hi')} / ${str(reasonKey, 'en')}`;
  const res = await alex.requestCapability(name, reason);
  if (res && res.granted) {
    granted.add(name);
    return true;
  }
  return false;
}

function pickRecorderMime() {
  if (typeof MediaRecorder === 'undefined') return null;
  const candidates = ['audio/webm;codecs=opus', 'audio/webm', 'audio/mp4', 'audio/ogg;codecs=opus', 'audio/ogg'];
  if (typeof MediaRecorder.isTypeSupported !== 'function') return '';
  return candidates.find((m) => MediaRecorder.isTypeSupported(m)) ?? '';
}

async function startRecording(i) {
  const u = ui(i);
  u.error = null;
  if (activeRecorder) stopRecording();
  tts.stop();
  const mime = pickRecorderMime();
  if (mime === null || !navigator.mediaDevices?.getUserMedia) {
    u.error = bi('recorder_unavailable');
    render();
    return;
  }
  try {
    if (!(await ensureCapability('microphone', 'mic_reason'))) {
      u.error = bi('mic_denied');
      render();
      return;
    }
    const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
    const recorder = mime ? new MediaRecorder(stream, { mimeType: mime }) : new MediaRecorder(stream);
    const chunks = [];
    recorder.addEventListener('dataavailable', (e) => {
      if (e.data && e.data.size > 0) chunks.push(e.data);
    });
    recorder.addEventListener('stop', () => {
      for (const track of stream.getTracks()) track.stop();
      const type = recorder.mimeType || mime || 'audio/webm';
      const blob = new Blob(chunks, { type });
      if (blob.size > 0) {
        const prev = recordings.get(i);
        if (prev) URL.revokeObjectURL(prev.url);
        recordings.set(i, { blob, url: URL.createObjectURL(blob), mime: type.split(';')[0] });
        state.recorded[i] = true;
        state.sent[i] = false;
        state.checks[i] = null;
        Object.assign(ui(i), { packMissing: false, unavailable: false, error: null });
        persist();
        maybeComplete();
      }
      render();
    });
    recorder.start();
    const startedAt = Date.now();
    activeRecorder = {
      index: i,
      recorder,
      stream,
      startedAt,
      timer: setTimeout(stopRecording, MAX_RECORD_MS),
      tick: setInterval(() => {
        const el = document.getElementById(`rec-time-${i}`);
        if (el) el.replaceChildren(bi('recording', { s: Math.floor((Date.now() - startedAt) / 1000) }));
      }, 500),
    };
  } catch (e) {
    u.error = bi('mic_error', { error: errText(e) });
  }
  render();
}

function stopRecording() {
  const a = activeRecorder;
  if (!a) return;
  activeRecorder = null;
  clearTimeout(a.timer);
  clearInterval(a.tick);
  try {
    if (a.recorder.state !== 'inactive') a.recorder.stop();
    else for (const track of a.stream.getTracks()) track.stop();
  } catch (_) {
    for (const track of a.stream.getTracks()) track.stop();
  }
  render();
}

function maybeComplete() {
  const n = items().length;
  if (n === 0 || state.completed) return;
  for (let i = 0; i < n; i++) if (!state.recorded[i]) return;
  state.completed = true;
  persist();
  void alex.complete(1).catch(() => {});
}

async function decodeToPcm16Base64(blob) {
  const AC = window.AudioContext || window.webkitAudioContext;
  const OAC = window.OfflineAudioContext || window.webkitOfflineAudioContext;
  if (!AC || !OAC) throw new Error('Web Audio is not available');
  const data = await blob.arrayBuffer();
  const ctx = new AC();
  let decoded;
  try {
    decoded = await new Promise((resolve, reject) => {
      const p = ctx.decodeAudioData(data, resolve, reject);
      if (p && typeof p.then === 'function') p.then(resolve, reject);
    });
  } finally {
    void ctx.close?.();
  }
  const length = Math.max(1, Math.ceil(decoded.duration * SPEECH_SAMPLE_RATE));
  const offline = new OAC(1, length, SPEECH_SAMPLE_RATE);
  const src = offline.createBufferSource();
  src.buffer = decoded;
  src.connect(offline.destination);
  src.start(0);
  const rendered = await new Promise((resolve, reject) => {
    offline.oncomplete = (e) => resolve(e.renderedBuffer);
    const p = offline.startRendering();
    if (p && typeof p.then === 'function') p.then(resolve, reject);
  });
  return bytesToBase64(floatToPcm16LE(rendered.getChannelData(0)));
}

async function checkPronunciation(i, text) {
  const rec = recordings.get(i);
  const u = ui(i);
  if (!rec || u.busy) return;
  Object.assign(u, { error: null, packMissing: false, unavailable: false, busy: 'check' });
  render();
  try {
    if (!(await ensureCapability('ml_inference', 'ml_reason'))) {
      u.error = bi('ml_denied');
      return;
    }
    const pcm16 = await decodeToPcm16Base64(rec.blob);
    const res = await alex.emitEvent('speech_check', {
      lang: typeof content.lang === 'string' ? content.lang : 'en',
      text,
      pcm16_b64: pcm16,
      sample_rate: SPEECH_SAMPLE_RATE,
    });
    if (!res || typeof res !== 'object' || !Array.isArray(res.words)) {
      u.unavailable = true;
      return;
    }
    state.checks[i] = {
      heard: typeof res.heard === 'string' ? res.heard : '',
      overall: typeof res.overall === 'number' ? res.overall : null,
      words: res.words
        .filter((w) => w && typeof w.word === 'string')
        .map((w) => ({
          word: w.word,
          status: ['clear', 'unclear', 'missed'].includes(w.status) ? w.status : 'unclear',
        })),
    };
    persist();
  } catch (e) {
    const msg = errText(e);
    if (msg.includes('speech_pack_missing')) u.packMissing = true;
    else u.error = bi('check_failed', { error: msg });
  } finally {
    u.busy = null;
    render();
  }
}

async function sendToInstructor(i, text) {
  const rec = recordings.get(i);
  const u = ui(i);
  if (!rec || u.busy) return;
  Object.assign(u, { error: null, busy: 'send' });
  render();
  try {
    const bytes = new Uint8Array(await rec.blob.arrayBuffer());
    await alex.submit(
      {
        files: [
          {
            name: `item-${i + 1}.${extForMime(rec.mime)}`,
            mime: rec.mime,
            size: rec.blob.size,
            data_b64: bytesToBase64(bytes),
          },
        ],
        comment: `Read aloud: "${text}"`,
        text,
      },
      { type: 'irl_review', skills: Array.isArray(content.skills) ? content.skills : [] },
    );
    state.sent[i] = true;
    persist();
  } catch (e) {
    u.error = bi('send_failed', { error: errText(e) });
  } finally {
    u.busy = null;
    render();
  }
}

function checkResult(i) {
  const u = ui(i);
  const res = state.checks[i];
  if (u.packMissing) {
    return h(
      'div',
      { class: 'notice warn' },
      bi('pack_missing'),
      h('div', { class: 'actions' }, h('button', {
        type: 'button',
        class: 'primary',
        onclick: () => void alex.emitEvent('open_plugin_settings', {}).catch(() => {}),
      }, bi('open_settings'))),
    );
  }
  if (u.unavailable) return h('div', { class: 'notice' }, bi('check_unavailable'));
  if (!res) return null;
  const legend = h(
    'div',
    { class: 'legend' },
    h('span', { class: 'w clear' }, t('status_clear')),
    h('span', { class: 'w unclear' }, t('status_unclear')),
    h('span', { class: 'w missed' }, t('status_missed')),
  );
  return h(
    'div',
    { class: 'result' },
    typeof res.overall === 'number'
      ? h('div', {}, h('span', { class: 'score-badge' }, bi('clarity', { pct: Math.round(Math.max(0, Math.min(1, res.overall)) * 100) })))
      : null,
    h('div', { class: 'words', lang: 'en' }, res.words.map((w) => h('span', { class: `w ${w.status}`, title: t(`status_${w.status}`) }, w.word))),
    legend,
    res.heard ? h('div', { class: 'heard' }, h('b', {}, t('heard') + ': '), h('span', { lang: 'en' }, `“${res.heard}”`)) : null,
    h('div', { class: 'disclaimer' }, bi('disclaimer')),
  );
}

function renderReadAloud() {
  const list = h('div', { class: 'items' });
  items().forEach((item, i) => {
    const text = String(item?.text ?? '');
    const rec = recordings.get(i);
    const u = ui(i);
    const isRecording = activeRecorder && activeRecorder.index === i;

    const recordBtn = isRecording
      ? h('button', { type: 'button', class: 'danger', onclick: stopRecording }, '■ ', bi('stop_recording'))
      : h('button', {
        type: 'button',
        class: rec || state.recorded[i] ? '' : 'primary',
        disabled: Boolean(u.busy),
        onclick: () => void startRecording(i),
      }, '🎙 ', bi(rec || state.recorded[i] ? 'record_again' : 'record'));

    const card = h(
      'section',
      { class: 'card' },
      h('div', { class: 'card-label' }, bi('item_n', { n: i + 1 })),
      h('p', { class: 'say', lang: 'en' }, text),
      hintEl(item?.hint_hi),
      h('div', { class: 'row' }, ...listenButtons(text), recordBtn),
    );
    if (isRecording) {
      card.append(
        h('div', { class: 'row status', id: `rec-time-${i}` }, h('span', { class: 'rec-dot', 'aria-hidden': 'true' }),
          bi('recording', { s: Math.floor((Date.now() - activeRecorder.startedAt) / 1000) })),
      );
    }
    if (rec) {
      card.append(
        h(
          'div',
          { class: 'compare' },
          h('div', {}, h('div', { class: 'label' }, bi('model_audio')), h('div', { class: 'row' }, ...listenButtons(text).slice(0, 1))),
          h('div', {}, h('div', { class: 'label' }, bi('your_recording')), h('audio', { controls: true, src: rec.url, preload: 'metadata' })),
        ),
        h(
          'div',
          { class: 'row' },
          h('button', { type: 'button', disabled: Boolean(u.busy) || isRecording, onclick: () => void checkPronunciation(i, text) },
            '✓ ', bi(u.busy === 'check' ? 'checking' : 'check_pronunciation')),
          state.sent[i]
            ? h('span', { class: 'status' }, '✔ ', bi('sent'))
            : h('button', { type: 'button', disabled: Boolean(u.busy) || isRecording, onclick: () => void sendToInstructor(i, text) },
              '✉ ', bi(u.busy === 'send' ? 'sending' : 'send_instructor')),
        ),
      );
    } else if (state.recorded[i] && !isRecording) {
      card.append(h('div', { class: 'hint' }, bi('recorded_earlier')));
      if (state.sent[i]) card.append(h('div', { class: 'row status' }, '✔ ', bi('sent')));
    }
    if (u.error) card.append(h('div', { class: 'notice bad', style: 'margin-top:10px' }, u.error));
    const result = checkResult(i);
    if (result) card.append(result);
    list.append(card);
  });
  root.append(list);
  if (state.completed) root.append(h('div', { class: 'notice ok', style: 'margin-top:14px' }, bi('all_recorded')));
}

const RENDERERS = {
  dictation: renderDictation,
  reading: renderReading,
  writing: renderWriting,
  builder: renderBuilder,
  read_aloud: renderReadAloud,
};

// ------------------------------------------------------------- host wiring

alex.onHost((msg) => {
  if (!msg || typeof msg !== 'object') return;
  const payload = msg.payload && typeof msg.payload === 'object' ? msg.payload : {};
  if (msg.type === 'init') {
    content = payload.content && typeof payload.content === 'object' ? payload.content : {};
    settings = readSettings(payload.settings);
    state = restoreState(payload.state, String(content.activity ?? ''));
    lang = state.lang === 'en' || state.lang === 'hi' ? state.lang : settings.ui_language;
    granted = new Set(Array.isArray(payload.granted_capabilities) ? payload.granted_capabilities : []);
    render();
  } else if (msg.type === 'settings_changed') {
    settings = readSettings(payload.settings ?? payload);
    if (!state.lang) lang = settings.ui_language;
    render();
  } else if (msg.type === 'capability_granted' && payload.name) {
    granted.add(payload.name);
  } else if (msg.type === 'capability_revoked' && payload.name) {
    granted.delete(payload.name);
    if (payload.name === 'microphone') stopRecording();
  } else if (msg.type === 'submit_ack') {
    // The awaited submit promise already carries the score; this only
    // covers a host that acks without resolving.
    if (!state.submitted && !submitting && typeof payload.score === 'number') onScored(payload.score);
  }
});

tts.load();
void alex.ready(['microphone', 'ml_inference']);
