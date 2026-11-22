// Smoke tests for the Language Lab UI: mount app.js against a fake
// `window.alex`, send `init` for each activity, and drive the main flows.
// Audio, recording and speech synthesis are browser-only and not exercised.

import { describe, it, expect, beforeEach, vi } from 'vitest';

let hostHandler = null;
let calls = [];

function fakeAlex(overrides = {}) {
  return {
    onHost: (fn) => {
      hostHandler = fn;
    },
    ready: vi.fn(async () => null),
    persistState: vi.fn(async () => null),
    complete: vi.fn(async () => null),
    requestCapability: vi.fn(async () => ({ granted: true })),
    emitEvent: vi.fn(async () => null),
    submit: vi.fn(async (submission, metadata) => {
      calls.push({ submission, metadata });
      return { score: 0.75 };
    }),
    ...overrides,
  };
}

async function mount(content, extra = {}) {
  document.body.innerHTML = '<main id="app"></main>';
  hostHandler = null;
  calls = [];
  vi.resetModules();
  globalThis.alex = fakeAlex(extra.alex);
  await import('./app.js');
  hostHandler({
    type: 'init',
    payload: { content, state: extra.state ?? null, settings: extra.settings, granted_capabilities: [] },
  });
  return document.getElementById('app');
}

const click = (el) => el.dispatchEvent(new MouseEvent('click', { bubbles: true }));
const flush = () => new Promise((r) => setTimeout(r, 0));
const buttonWithText = (root, text) =>
  [...root.querySelectorAll('button')].find((b) => b.textContent.includes(text));

describe('Language Lab UI', () => {
  beforeEach(() => {
    vi.useRealTimers();
  });

  it('renders a dictation activity bilingually and submits answers', async () => {
    const root = await mount({
      activity: 'dictation',
      items: [{ text: 'Good morning.', hint_hi: 'सुप्रभात' }, { text: 'Thank you.' }],
    });
    expect(root.textContent).toContain('श्रुतलेख');
    expect(root.textContent).toContain('Dictation');
    expect(root.textContent).toContain('सुप्रभात');
    const inputs = root.querySelectorAll('input[type="text"]');
    expect(inputs).toHaveLength(2);
    inputs[0].value = 'good morning';
    inputs[0].dispatchEvent(new Event('input'));
    click(buttonWithText(root, 'Submit'));
    await flush();
    expect(calls[0].submission).toEqual({ answers: ['good morning', ''] });
    expect(calls[0].metadata).toEqual({ activity: 'dictation' });
    expect(root.textContent).toContain('75%');
    // The local diff marks the missing word of item 2.
    expect(root.querySelector('.w.missing')?.textContent).toBe('thank');
    expect(globalThis.alex.complete).toHaveBeenCalledWith(1, 0.75);
  });

  it('switches to English only and hides hints by setting', async () => {
    const root = await mount(
      { activity: 'dictation', items: [{ text: 'Hi.', hint_hi: 'नमस्ते' }] },
      { settings: { ui_language: 'en', show_hints: false } },
    );
    expect(root.textContent).toContain('Dictation');
    expect(root.textContent).not.toContain('श्रुतलेख');
    expect(root.textContent).not.toContain('नमस्ते');
  });

  it('shows glossary meanings and submits reading choices', async () => {
    const root = await mount({
      activity: 'reading',
      passage: 'Ravi has a bicycle.',
      glossary: { bicycle: 'साइकिल' },
      questions: [{ prompt: 'What does Ravi have?', options: ['A car', 'A bicycle'] }],
    });
    const gloss = root.querySelector('.gloss');
    expect(gloss.textContent).toBe('bicycle');
    click(gloss);
    expect(root.querySelector('.popover').textContent).toContain('साइकिल');
    const radios = root.querySelectorAll('input[type="radio"]');
    radios[1].checked = true;
    radios[1].dispatchEvent(new Event('change'));
    click(buttonWithText(root, 'Submit'));
    await flush();
    expect(calls[0].submission).toEqual({ selected: [1] });
  });

  it('builds a sentence from tiles', async () => {
    const root = await mount({ activity: 'builder', items: [{ tiles: ['rice.', 'eat', 'I'] }] });
    for (const word of ['I', 'eat', 'rice.']) {
      const tile = [...root.querySelectorAll('.bank .tile')].find((b) => b.textContent === word && !b.disabled);
      click(tile);
    }
    expect([...root.querySelectorAll('.built .tile')].map((b) => b.textContent)).toEqual(['I', 'eat', 'rice.']);
    click(buttonWithText(root, 'Submit'));
    await flush();
    expect(calls[0].submission).toEqual({ orders: [[2, 1, 0]] });
  });

  it('shows live writing tips', async () => {
    vi.useFakeTimers();
    const root = await mount({ activity: 'writing', items: [{ prompt_hi: 'मेरे पास एक छाता है।' }] });
    const area = root.querySelector('textarea');
    area.value = 'i have a umbrella';
    area.dispatchEvent(new Event('input'));
    vi.advanceTimersByTime(400);
    const tips = root.querySelector('.tips').textContent;
    expect(tips).toContain('capital letter');
    expect(tips).toContain('Use "an" before "umbrella"');
  });

  it('restores read-aloud progress and the last pronunciation check', async () => {
    const root = await mount(
      { activity: 'read_aloud', items: [{ text: 'Very well, thank you.' }] },
      {
        state: {
          v: 1,
          activity: 'read_aloud',
          recorded: [true],
          sent: [true],
          checks: [{ heard: 'very well thank you', overall: 0.9, words: [{ word: 'very', status: 'clear' }] }],
        },
      },
    );
    expect(root.textContent).toContain('Very well, thank you.');
    expect(root.textContent).toContain('Record again');
    expect(root.textContent).toContain('Clarity: 90%');
    expect(root.querySelector('.w.clear')?.textContent).toBe('very');
    expect(root.textContent).toContain('does not judge your accent');
  });

  it('flags an unknown activity', async () => {
    const root = await mount({ activity: 'poetry' });
    expect(root.querySelector('.notice.bad')).not.toBeNull();
  });
});
