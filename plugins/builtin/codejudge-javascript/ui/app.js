// codejudge: JavaScript — plugin UI harness.
//
// Interactive plugin (no host grader): the learner writes JS, we run it locally
// in a QuickJS wasm sandbox (runner.js) against the problem's test cases and
// report pass/fail. The QuickJS module loads asynchronously, so Run/Submit stay
// disabled until the runtime is ready. Hidden tests never reveal their data.

(function () {
  "use strict";

  var DEFAULT_STUB =
    "// Read input from the `stdin` string (or readLine()); print with console.log.\n" +
    "// const n = +readLine();\n" +
    "// console.log(answer);\n\n";

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
  var runtime = null; // { run } once QuickJS is ready
  var saveTimer = null;

  function normalize(t) {
    var lines = (t || "").replace(/\r\n/g, "\n").split("\n").map(function (l) {
      return l.replace(/\s+$/, "");
    });
    while (lines.length && lines[lines.length - 1] === "") lines.pop();
    return lines.join("\n");
  }

  function mdToHtml(s) {
    var esc = String(s || "")
      .replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
    return esc
      .replace(/\*\*([^*]+)\*\*/g, "<b>$1</b>")
      .replace(/`([^`]+)`/g, "<code>$1</code>")
      .replace(/\n/g, "<br>");
  }

  function escapeText(s) {
    return String(s == null ? "" : s)
      .replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
  }

  function setupEditor(initial) {
    var ta = document.getElementById("code");
    ta.value = initial;
    editor = CodeMirror.fromTextArea(ta, {
      mode: "javascript",
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

  function runCases(cases) {
    var source = editor.getValue();
    var limitMs = (problem.limits && problem.limits.time_ms) || 2000;
    var passed = 0;
    var details = [];
    for (var i = 0; i < cases.length; i++) {
      var c = cases[i];
      var r = runtime.run(source, c.input, { timeoutMs: limitMs });
      var ok = !r.error && normalize(r.stdout) === normalize(c.output);
      if (ok) passed++;
      details.push({ index: i, ok: ok, status: r.timedOut ? "TLE" : r.error ? "RE" : ok ? "AC" : "WA",
        expected: c.output, got: r.stdout, error: r.error });
    }
    return { passed: passed, total: cases.length, details: details };
  }

  function renderResults(title, res, revealUpTo) {
    els.results.innerHTML = "";
    var v = document.createElement("div");
    var allOk = res.passed === res.total;
    v.className = "verdict " + (allOk ? "ok" : "bad");
    v.textContent = title + ": " + res.passed + "/" + res.total + " passed" + (allOk ? "  ✓" : "");
    els.results.appendChild(v);

    res.details.forEach(function (d) {
      var row = document.createElement("div");
      row.className = "case " + (d.ok ? "ok" : "bad");
      var reveal = revealUpTo === undefined || d.index < revealUpTo;
      var detail = "";
      if (!d.ok && reveal) {
        if (d.error) detail = "  " + d.error;
        else detail = "  expected " + JSON.stringify(normalize(d.expected)) +
          ", got " + JSON.stringify(normalize(d.got));
      }
      row.innerHTML = '<span class="mark">' + (d.ok ? "✓" : "✗") + "</span>" +
        "<span>test " + (d.index + 1) + ": " + d.status + '</span><span class="detail">' +
        escapeText(detail) + "</span>";
      els.results.appendChild(row);
    });
  }

  function onRun() {
    var visible = (problem.tests && problem.tests.visible) || [];
    renderResults("Sample tests", runCases(visible));
  }

  function onSubmit() {
    var visible = (problem.tests && problem.tests.visible) || [];
    var all = visible.concat((problem.tests && problem.tests.hidden) || []);
    var res = runCases(all);
    renderResults("Submission", res, visible.length); // reveal only visible cases
    var frac = res.total ? res.passed / res.total : 0;
    try { alex.complete(frac, frac); } catch (e) {}
  }

  function resolveProblem(content, state) {
    var bank = window.CODEJUDGE_PROBLEMS || {};
    var p = null;
    if (content && content.problem) p = content.problem;
    else if (content && content.problem_id && bank[content.problem_id]) p = bank[content.problem_id];
    if (!p) { var keys = Object.keys(bank); if (keys.length) p = bank[keys[0]]; }
    if (!p) p = { id: "blank", title: "No problem", statement_md: "No problem content was provided.", tests: { visible: [], hidden: [] } };
    problem = p;
    renderProblem();
    setupEditor((content && content.starter_code) || (state && state.source) || DEFAULT_STUB);
    initRuntime();
  }

  function initRuntime() {
    els.results.textContent = "Loading JavaScript runtime…";
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
      resolveProblem(window.__CODEJUDGE_PREVIEW || {}, {});
    }
  }

  els.run.addEventListener("click", onRun);
  els.submit.addEventListener("click", onSubmit);
  boot();
})();
