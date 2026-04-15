// Built-in MCQ plugin UI (Phase 2).
//
// Renders one MCQ from the content envelope, lets the learner pick
// their answer(s), and submits. Grading itself happens host-side in the
// deterministic Wasmtime sandbox via mcq-grader.wasm — this script only
// captures the selection. Without `correct_indices` in the content
// (those are stripped on the host before init), the UI cannot reveal
// the answer key locally.

(function () {
  'use strict';

  const promptEl = document.getElementById('prompt');
  const optionsEl = document.getElementById('options');
  const submitBtn = document.getElementById('submit');
  const scoreEl = document.getElementById('score');
  const errorEl = document.getElementById('error');

  let mode = 'single';
  let optionCount = 0;
  let locked = false;

  alex.onHost(handleHostMessage);

  function handleHostMessage(msg) {
    if (!msg || typeof msg !== 'object') return;
    if (msg.type === 'init') {
      const content = (msg.payload && msg.payload.content) || {};
      mode = content.kind === 'multi' ? 'multi' : 'single';
      const options = Array.isArray(content.options) ? content.options : [];
      optionCount = options.length;
      const prompt = typeof content.prompt === 'string' ? content.prompt : 'Choose:';
      promptEl.textContent = prompt;
      renderOptions(options, mode);
      submitBtn.disabled = false;
    } else if (msg.type === 'submit_ack') {
      const score = msg.payload && typeof msg.payload.score === 'number'
        ? msg.payload.score
        : null;
      if (score !== null) {
        scoreEl.textContent = `Score: ${Math.round(score * 100)}%`;
        for (const opt of optionsEl.querySelectorAll('.option')) {
          opt.classList.add('locked');
        }
      } else {
        scoreEl.textContent = 'Submitted.';
      }
      void alex.complete(1, score);
    }
  }

  function renderOptions(options, kind) {
    optionsEl.innerHTML = '';
    optionsEl.setAttribute('role', kind === 'multi' ? 'group' : 'radiogroup');
    options.forEach((label, idx) => {
      const wrapper = document.createElement('label');
      wrapper.className = 'option';

      const input = document.createElement('input');
      input.type = kind === 'multi' ? 'checkbox' : 'radio';
      input.name = 'mcq';
      input.value = String(idx);
      input.dataset.index = String(idx);

      input.addEventListener('change', () => {
        for (const opt of optionsEl.querySelectorAll('.option')) {
          opt.classList.toggle(
            'selected',
            opt.querySelector('input')?.checked === true,
          );
        }
      });

      const span = document.createElement('span');
      span.textContent = label;

      wrapper.append(input, span);
      optionsEl.append(wrapper);
    });
  }

  submitBtn.addEventListener('click', async () => {
    if (locked) return;
    const selected = [];
    for (const inp of optionsEl.querySelectorAll('input')) {
      if (inp.checked) selected.push(parseInt(inp.dataset.index, 10));
    }
    if (selected.length === 0) {
      errorEl.textContent = 'Pick at least one answer.';
      return;
    }
    errorEl.textContent = '';
    submitBtn.disabled = true;
    locked = true;

    try {
      // The host runs mcq-grader.wasm and returns a SubmitAck via onHost.
      await alex.submit({ selected_indices: selected }, { kind: mode });
    } catch (err) {
      errorEl.textContent = `Submit failed: ${err.message || err}`;
      submitBtn.disabled = false;
      locked = false;
    }
  });

  // Hand off to the host: we declare that we use no capabilities.
  void alex.ready([]);
})();
