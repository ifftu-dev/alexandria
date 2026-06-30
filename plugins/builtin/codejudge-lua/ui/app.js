// codejudge: Lua — plugin UI harness.
//
// Interactive plugin (no host grader): the learner writes Lua, we run it
// locally in the wasmoon Lua VM (runner.js) against the problem's test cases
// and report pass/fail. The wasm runtime loads asynchronously, so Run/Submit
// stay disabled until it's ready and each run is awaited. Progress goes to the
// host via alex.complete(frac); hidden tests reveal only pass/fail counts.

(function () {
  "use strict";

  var DEFAULT_STUB =
    "-- Read input with io.read(); print() your answer.\n" +
    "-- io.read(\"l\") -> next line, io.read(\"n\") -> next number, io.read(\"a\") -> all.\n\n";

  var els = {
    title: document.getElementById("title"),
    difficulty: document.getElementById("difficulty"),
    statement: document.getElementById("statement"),
    samples: document.getElementById("samples"),
    results: document.getElementById("results"),
    run: document.getElementById("run"),
    submit: document.getElementById("submit"),
  };

  var problem = null;
  var editor = null;
  var runtime = null; // { run } once the wasm Lua runtime is ready
  var saveTimer = null;

  // --- output comparison (identical rule across all codejudge plugins) ---
  function normalize(t) {
    var lines = (t || "").replace(/\r\n/g, "\n").split("\n").map(function (l) {
      return l.replace(/\s+$/, "");
    });
    while (lines.length && lines[lines.length - 1] === "") lines.pop();
    return lines.join("\n");
  }

  // --- minimal, safe markdown (escape first; then **bold**, `code`) ---
  function mdToHtml(s) {
    var esc = String(s || "")
      .replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
    return esc
      .replace(/\*\*([^*]+)\*\*/g, "<b>$1</b>")
      .replace(/`([^`]+)`/g, "<code>$1</code>")
      .replace(/\n/g, "<br>");
  }

  function setupEditor(initial) {
    var ta = document.getElementById("code");
    ta.value = initial;
    editor = CodeMirror.fromTextArea(ta, {
      mode: "lua",
      theme: "material-darker",
      lineNumbers: true,
      lineWrapping: true,
      indentUnit: 2,
      tabSize: 2,
      matchBrackets: true,
    });
    editor.on("change", function () {
      clearTimeout(saveTimer);
      saveTimer = setTimeout(function () {
        try { alex.persistState({ source: editor.getValue() }); } catch (e) {}
      }, 600);
    });
  }

  function renderProblem() {
    els.title.textContent = problem.title || problem.id;
    els.difficulty.textContent = problem.difficulty || "";
    els.statement.innerHTML = mdToHtml(problem.statement_md || "");
    els.samples.innerHTML = "";
    (problem.tests && problem.tests.visible ? problem.tests.visible : []).forEach(
      function (c, i) {
        var div = document.createElement("div");
        div.className = "sample";
        div.innerHTML =
          "<b>sample " + (i + 1) + " — input</b>\n" + escapeText(c.input) +
          "\n<b>expected</b>\n" + escapeText(c.output) +
          (c.explain ? "\n<b>note</b> " + escapeText(c.explain) : "");
        els.samples.appendChild(div);
      }
    );
  }

  function escapeText(s) {
    return String(s == null ? "" : s)
      .replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
  }

  // Run a set of cases; returns { passed, total, firstError, details[] }.
  // Async: wasmoon's run is awaited per case.
  async function runCases(cases, reveal) {
    var source = editor.getValue();
    var limitMs = (problem.limits && problem.limits.time_ms) || 2000;
    var passed = 0;
    var details = [];
    var firstError = null;
    for (var i = 0; i < cases.length; i++) {
      var c = cases[i];
      var r = await runtime.run(source, c.input, { timeoutMs: limitMs });
      var ok = !r.error && normalize(r.stdout) === normalize(c.output);
      if (ok) passed++;
      else if (!firstError && r.error) firstError = r.error;
      details.push({ index: i, ok: ok, status: r.timedOut ? "TLE" : r.error ? "RE" : ok ? "AC" : "WA",
        input: c.input, expected: c.output, got: r.stdout, error: r.error, reveal: reveal });
    }
    return { passed: passed, total: cases.length, firstError: firstError, details: details };
  }

  function renderResults(title, res) {
    els.results.innerHTML = "";
    var v = document.createElement("div");
    var allOk = res.passed === res.total;
    v.className = "verdict " + (allOk ? "ok" : "bad");
    v.textContent = title + ": " + res.passed + "/" + res.total + " passed" +
      (allOk ? "  ✓" : "");
    els.results.appendChild(v);

    res.details.forEach(function (d) {
      var row = document.createElement("div");
      row.className = "case " + (d.ok ? "ok" : "bad");
      var mark = d.ok ? "✓" : "✗";
      var label = "test " + (d.index + 1) + ": " + d.status;
      var detail = "";
      if (!d.ok && d.reveal) {
        if (d.error) detail = "  " + d.error;
        else detail = "  expected " + JSON.stringify(normalize(d.expected)) +
          ", got " + JSON.stringify(normalize(d.got));
      }
      row.innerHTML = '<span class="mark">' + mark + "</span>" +
        "<span>" + label + '</span><span class="detail">' + escapeText(detail) + "</span>";
      els.results.appendChild(row);
    });
  }

  function setBusy(b) {
    els.run.disabled = b || !runtime;
    els.submit.disabled = b || !runtime;
  }

  async function onRun() {
    if (!runtime) return;
    var visible = (problem.tests && problem.tests.visible) || [];
    setBusy(true);
    els.results.textContent = "Running…";
    try {
      renderResults("Sample tests", await runCases(visible, true));
    } finally {
      setBusy(false);
    }
  }

  async function onSubmit() {
    if (!runtime) return;
    var all = ((problem.tests && problem.tests.visible) || [])
      .concat((problem.tests && problem.tests.hidden) || []);
    var visibleCount = ((problem.tests && problem.tests.visible) || []).length;
    setBusy(true);
    els.results.textContent = "Running…";
    try {
      var res = await runCases(all, false);
      // Reveal details only for visible cases; hidden cases show pass/fail only.
      res.details.forEach(function (d) { d.reveal = d.index < visibleCount; });
      renderResults("Submission", res);
      var frac = res.total ? res.passed / res.total : 0;
      try { alex.complete(frac, frac); } catch (e) {}
    } finally {
      setBusy(false);
    }
  }

  // --- host bridge (with a standalone fallback for dev/preview) ---
  function resolveProblem(content, state) {
    var bank = window.CODEJUDGE_PROBLEMS || {};
    var p = null;
    if (content && content.problem) p = content.problem;
    else if (content && content.problem_id && bank[content.problem_id]) p = bank[content.problem_id];
    if (!p) {
      var keys = Object.keys(bank);
      if (keys.length) p = bank[keys[0]];
    }
    if (!p) p = { id: "blank", title: "No problem", statement_md: "No problem content was provided.", tests: { visible: [], hidden: [] } };
    problem = p;

    var starter = (content && content.starter_code) ||
      (state && state.source) || DEFAULT_STUB;
    renderProblem();
    setupEditor(starter);
    initRuntime();
  }

  function initRuntime() {
    els.run.disabled = true;
    els.submit.disabled = true;
    els.results.textContent = "Loading Lua runtime…";
    CodejudgeRunnerInit().then(function (rt) {
      runtime = rt;
      els.results.textContent = "";
      els.run.disabled = false;
      els.submit.disabled = false;
    }).catch(function (e) {
      els.results.innerHTML = '<div class="error">Failed to load runtime: ' +
        escapeText(e && e.message ? e.message : String(e)) + "</div>";
    });
  }

  function boot() {
    if (typeof window.alex !== "undefined") {
      alex.onHost(function (msg) {
        if (!msg || msg.type !== "init") return;
        var payload = msg.payload || {};
        resolveProblem(payload.content || {}, payload.state || {});
      });
      void alex.ready([]);
    } else {
      // Standalone preview: no host. Use the bundled bank + a global override
      // window.__CODEJUDGE_PREVIEW = { problem_id } if a page wants to pick one.
      var pre = window.__CODEJUDGE_PREVIEW || {};
      resolveProblem(pre, {});
    }
  }

  els.run.addEventListener("click", onRun);
  els.submit.addEventListener("click", onSubmit);
  boot();
})();
