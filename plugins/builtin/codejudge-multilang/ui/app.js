// codejudge (umbrella) — landing UI.
//
// This plugin runs no code itself; it is the umbrella that declares the
// per-language judge plugins as dependencies (so installing it installs them
// all) and presents the shared problem bank + the available languages. Each
// language is a separate installed element type.

(function () {
  "use strict";

  // Languages this suite ships, in dependency order. `available` mirrors which
  // dependency plugins exist today; Python lands with its Pyodide runtime.
  var LANGUAGES = [
    { name: "JavaScript", runtime: "QuickJS (WebAssembly)", available: true },
    { name: "Lua", runtime: "fengari (pure-JS Lua VM)", available: true },
    { name: "Python", runtime: "Pyodide", available: false },
  ];

  function el(tag, cls, text) {
    var e = document.createElement(tag);
    if (cls) e.className = cls;
    if (text != null) e.textContent = text;
    return e;
  }

  function renderLanguages() {
    var host = document.getElementById("languages");
    host.innerHTML = "";
    LANGUAGES.forEach(function (l) {
      var card = el("div", "card" + (l.available ? "" : " soon"));
      card.appendChild(el("div", "card-name", l.name));
      card.appendChild(el("div", "card-runtime", l.runtime));
      card.appendChild(el("div", "card-tag", l.available ? "runs locally" : "coming soon"));
      host.appendChild(card);
    });
  }

  function renderProblems() {
    var bank = window.CODEJUDGE_PROBLEMS || {};
    var body = document.getElementById("problems");
    body.innerHTML = "";
    Object.keys(bank).sort().forEach(function (id) {
      var p = bank[id];
      var tests =
        ((p.tests && p.tests.visible) || []).length +
        ((p.tests && p.tests.hidden) || []).length;
      var tr = el("tr");
      tr.appendChild(el("td", "p-title", p.title || id));
      var diff = el("td");
      diff.appendChild(el("span", "badge " + (p.difficulty || ""), p.difficulty || ""));
      tr.appendChild(diff);
      tr.appendChild(el("td", "p-tests", String(tests)));
      body.appendChild(tr);
    });
  }

  function boot() {
    renderLanguages();
    renderProblems();
    if (typeof window.alex !== "undefined") {
      alex.onHost(function (msg) {
        if (msg && msg.type === "init") { try { alex.complete(1); } catch (e) {} }
      });
      void alex.ready([]);
    }
  }

  boot();
})();
