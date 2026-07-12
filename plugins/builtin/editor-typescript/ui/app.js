// Code Editor: JavaScript / TypeScript — plugin UI.
//
// CodeMirror 6 editor + the Boa wasm engine (same artifact the host grades with)
// run on the iframe MAIN THREAD — the sandboxed opaque-origin iframe can't spawn
// a cross-origin plugin:// Worker. The wasm is loaded lazily (on first Run), so
// the editor and problem render immediately without waiting on the ~3MB module.

(function () {
  'use strict';

  var CM = self.AlexCM6;
  var IS_TS = !!(self.ALEX_EDITOR && self.ALEX_EDITOR.typescript);
  var LANG = IS_TS ? 'TypeScript' : 'JavaScript';
  var LIVE_DEBOUNCE_MS = 400;
  var SAVE_DEBOUNCE_MS = 600;
  var INIT_TIMEOUT_MS = 6000;

  var DEFAULT_SOURCE = IS_TS
    ? '// Write TypeScript. Types are stripped before running.\n' +
      '//   const n: number = Number(readLine());\n' +
      '//   console.log(n * 2);\n\n'
    : '// Write JavaScript. Print with console.log; read input with readLine().\n' +
      '//   const n = Number(readLine());\n' +
      '//   console.log(n * 2);\n\n';

  var els = {
    title: document.getElementById('title'),
    statement: document.getElementById('statement'),
    samples: document.getElementById('samples'),
    stdin: document.getElementById('stdin'),
    run: document.getElementById('run'),
    test: document.getElementById('test'),
    submit: document.getElementById('submit'),
    status: document.getElementById('status'),
    console: document.getElementById('console'),
    results: document.getElementById('results'),
    toolbar: document.querySelector('.toolbar'),
  };

  var view = null;
  var content = { tests: [] };
  var hasHost = typeof self.alex !== 'undefined';
  var started = false;
  var autorun = false;
  var liveTimer = null;
  var saveTimer = null;
  var initTimer = null;

  function esc(s) {
    return String(s == null ? '' : s)
      .replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
  }
  function md(s) {
    return esc(s)
      .replace(/\*\*([^*]+)\*\*/g, '<b>$1</b>')
      .replace(/`([^`]+)`/g, '<code>$1</code>')
      .replace(/\n/g, '<br>');
  }

  // ---- runner (main thread, lazy wasm load) ----
  function makeRunner() {
    var instance = null;
    var ready = null;

    function b64ToBytes(b64) {
      var bin = atob(b64);
      var n = bin.length;
      var a = new Uint8Array(n);
      for (var i = 0; i < n; i++) a[i] = bin.charCodeAt(i);
      return a;
    }

    function loadWasmScript() {
      // The base64 wasm is a ~4MB script; load it on demand so it never blocks
      // first paint. script-src 'self' allows the plugin:// origin.
      return new Promise(function (resolve, reject) {
        if (self.ALEX_RUNNER_WASM_B64) return resolve();
        var s = document.createElement('script');
        s.src = 'vendor/runner-wasm.js';
        s.onload = function () { resolve(); };
        s.onerror = function () { reject(new Error('failed to load runtime')); };
        document.head.appendChild(s);
      });
    }

    function init() {
      if (ready) return ready;
      ready = loadWasmScript()
        .then(function () { return WebAssembly.instantiate(b64ToBytes(self.ALEX_RUNNER_WASM_B64), {}); })
        .then(function (r) { instance = r.instance; });
      return ready;
    }

    function call(fn, payload) {
      var ex = instance.exports;
      var input = new TextEncoder().encode(JSON.stringify(payload));
      var ptr = ex.alex_alloc(input.length);
      if (ptr <= 0) throw new Error('alloc failed');
      new Uint8Array(ex.memory.buffer, ptr, input.length).set(input);
      var packed = ex[fn](ptr, input.length); // BigInt (i64)
      var outPtr = Number(packed >> 32n);
      var outLen = Number(packed & 0xffffffffn);
      var out = new Uint8Array(ex.memory.buffer, outPtr, outLen).slice();
      if (ex.alex_dealloc) { try { ex.alex_dealloc(ptr, input.length); } catch (_) {} }
      return JSON.parse(new TextDecoder().decode(out));
    }

    return {
      run: function (source, stdin) {
        return init().then(function () { return call('alex_run', { source: source, stdin: stdin }); });
      },
      grade: function (envelope) {
        return init().then(function () { return call('alex_grade', envelope); });
      },
    };
  }
  var runner = makeRunner();

  function sourceText() { return view ? view.state.doc.toString() : ''; }

  // ---- editor ----
  function makeEditor(initial) {
    if (view) { try { view.destroy(); } catch (_) {} view = null; }
    var parent = document.getElementById('editor');
    parent.innerHTML = '';
    var extensions = [
      CM.lineNumbers(),
      CM.highlightActiveLineGutter(),
      CM.history(),
      CM.drawSelection(),
      CM.dropCursor(),
      CM.indentOnInput(),
      CM.bracketMatching(),
      CM.closeBrackets(),
      CM.autocompletion(),
      CM.highlightActiveLine(),
      CM.highlightSelectionMatches(),
      CM.syntaxHighlighting(CM.defaultHighlightStyle, { fallback: true }),
      CM.javascript(IS_TS ? { typescript: true } : {}),
      CM.oneDark,
      CM.keymap.of(
        [].concat(
          CM.closeBracketsKeymap,
          CM.defaultKeymap,
          CM.historyKeymap,
          CM.completionKeymap,
          CM.searchKeymap,
          [CM.indentWithTab]
        )
      ),
      CM.EditorView.updateListener.of(function (u) {
        if (u.docChanged) onDocChanged();
      }),
    ];
    view = new CM.EditorView({
      parent: parent,
      state: CM.EditorState.create({ doc: initial, extensions: extensions }),
    });
    // The iframe can mount while its container is still 0-height (tab switch);
    // re-measure once layout settles so the editor isn't collapsed/blank.
    setTimeout(function () { try { view.requestMeasure(); } catch (_) {} }, 0);
    setTimeout(function () { try { view.requestMeasure(); } catch (_) {} }, 200);
  }

  function onDocChanged() {
    if (autorun) {
      clearTimeout(liveTimer);
      liveTimer = setTimeout(liveEval, LIVE_DEBOUNCE_MS);
    }
    clearTimeout(saveTimer);
    saveTimer = setTimeout(function () {
      if (hasHost) { try { self.alex.persistState({ source: sourceText() }); } catch (_) {} }
    }, SAVE_DEBOUNCE_MS);
  }

  // ---- actions ----
  function liveEval() {
    var src = sourceText();
    if (!src.trim()) { els.console.textContent = ''; return; }
    setStatus('running…');
    runner.run(src, els.stdin.value).then(function (r) {
      setStatus('');
      renderConsole(r);
    }, function (e) {
      setStatus('');
      els.console.textContent = 'runner error: ' + (e && e.message ? e.message : e);
    });
  }

  function renderConsole(r) {
    var out = (r.stdout || '');
    if (r.stderr) out += (out ? '\n' : '') + r.stderr;
    els.console.textContent = out || '(no output)';
  }

  function onRun() { liveEval(); }

  function onTest() {
    var visible = content.tests || [];
    if (!visible.length) { els.results.innerHTML = '<div class="muted">No visible tests.</div>'; return; }
    setStatus('running tests…');
    var envelope = { version: '1', content: { tests: visible }, submission: { source: sourceText() } };
    runner.grade(envelope).then(function (rec) {
      setStatus('');
      renderResults('Visible tests', rec);
    }, function (e) {
      setStatus('');
      els.results.innerHTML = '<div class="bad">Test run failed: ' + esc(e && e.message ? e.message : e) + '</div>';
    });
  }

  function renderResults(title, rec) {
    var d = rec.details || { passed: 0, total: 0, cases: [] };
    var allOk = d.total > 0 && d.passed === d.total;
    var html = '<div class="verdict ' + (allOk ? 'ok' : 'bad') + '">' +
      esc(title) + ': ' + d.passed + '/' + d.total + (allOk ? ' ✓' : '') + '</div>';
    (d.cases || []).forEach(function (c) {
      var detail = '';
      if (!c.passed && !c.hidden) {
        detail = c.error ? '  ' + c.error : '  got ' + JSON.stringify(c.got || '');
      }
      html += '<div class="case ' + (c.passed ? 'ok' : 'bad') + '">' +
        '<span class="mark">' + (c.passed ? '✓' : '✗') + '</span>' +
        '<span>' + esc(c.name) + (c.hidden ? ' (hidden)' : '') + '</span>' +
        '<span class="detail">' + esc(detail) + '</span></div>';
    });
    els.results.innerHTML = html;
  }

  function onSubmit() {
    if (!hasHost) { els.results.innerHTML = '<div class="muted">Submit requires the host.</div>'; return; }
    setStatus('grading… (first submit compiles the grader — may take a moment)');
    els.submit.disabled = true;
    self.alex.submit({ source: sourceText() }).then(function (res) {
      setStatus('');
      els.submit.disabled = false;
      var pct = Math.round((res && typeof res.score === 'number' ? res.score : 0) * 100);
      els.results.innerHTML = '<div class="verdict ' + (pct === 100 ? 'ok' : 'bad') + '">Graded: ' + pct + '%</div>';
    }, function (e) {
      setStatus('');
      els.submit.disabled = false;
      var msg = String(e && e.message ? e.message : e);
      els.results.innerHTML = '<div class="bad">' +
        (msg.indexOf('enrollment') >= 0
          ? 'Grading is only available for enrolled learners.'
          : (msg.indexOf('GraderUnavailable') >= 0 || msg.indexOf('desktop') >= 0
              ? 'Grading is available on desktop only; use Run tests here.'
              : 'Grading failed: ' + esc(msg))) +
        '</div>';
    });
  }

  function setStatus(s) { els.status.textContent = s; }

  // ---- instant-eval toggle (default OFF) ----
  function installToggle() {
    var label = document.createElement('label');
    label.className = 'toggle';
    var cb = document.createElement('input');
    cb.type = 'checkbox';
    cb.id = 'autorun';
    cb.checked = autorun;
    label.appendChild(cb);
    label.appendChild(document.createTextNode(' Evaluate instantly'));
    els.toolbar.appendChild(label);
    cb.addEventListener('change', function () {
      autorun = cb.checked;
      if (autorun) liveEval();
    });
  }

  // ---- render ----
  function render() {
    els.title.textContent = content.title || LANG;
    els.statement.innerHTML = md(content.prompt || content.statement_md || '');
    els.samples.innerHTML = '';
    var tests = content.tests || [];
    if (tests.length) {
      var h = document.createElement('div');
      h.className = 'tests-head';
      h.textContent = 'Tests';
      els.samples.appendChild(h);
    }
    tests.forEach(function (c, i) {
      var div = document.createElement('div');
      div.className = 'sample';
      div.innerHTML =
        '<b>' + esc(c.name || 'test ' + (i + 1)) + '</b>' +
        '\n<i>input</i>\n' + (c.stdin ? esc(c.stdin) : '(none)') +
        '\n<i>expected output</i>\n' + esc(c.expected_stdout);
      els.samples.appendChild(div);
    });
  }

  function start(payloadContent, state) {
    if (initTimer) { clearTimeout(initTimer); initTimer = null; }
    content = payloadContent && typeof payloadContent === 'object' ? payloadContent : { tests: [] };
    if (!content.tests) content.tests = [];
    try {
      render();
      makeEditor((content.starter_code) || (state && state.source) || DEFAULT_SOURCE);
    } catch (e) {
      els.results.innerHTML = '<div class="bad">Editor failed to load: ' + esc(e && e.message ? e.message : e) + '</div>';
      return;
    }
    els.run.disabled = false;
    els.test.disabled = false;
    els.submit.disabled = !hasHost;
    started = true;
  }

  // ---- boot ----
  installToggle();
  els.run.addEventListener('click', onRun);
  els.test.addEventListener('click', onTest);
  els.submit.addEventListener('click', onSubmit);

  if (hasHost) {
    els.statement.innerHTML = '<span class="muted">Loading…</span>';
    self.alex.onHost(function (msg) {
      if (!msg || msg.type !== 'init') return;
      var p = msg.payload || {};
      start(p.content || {}, p.state || {});
    });
    // Surface a stuck handshake instead of showing a blank pane forever.
    initTimer = setTimeout(function () {
      if (!started) {
        els.statement.innerHTML =
          '<span class="bad">No problem content received from the host. Try reopening this element.</span>';
        makeEditor(DEFAULT_SOURCE);
        els.run.disabled = false;
        els.test.disabled = false;
      }
    }, INIT_TIMEOUT_MS);
    void self.alex.ready([]);
  } else {
    start(self.__EDITOR_PREVIEW || {
      title: 'Preview',
      prompt: 'Standalone preview. `console.log` and `readLine()` work.',
      starter_code: 'console.log("hello from Boa");\n',
      tests: [{ name: 'demo', stdin: '', expected_stdout: 'hello from Boa' }],
    }, {});
  }
})();
