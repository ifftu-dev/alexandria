// Code Editors (collection) — landing UI.
//
// This plugin runs no code itself; it is the collection that declares the
// per-language editor plugins as dependencies (so installing it installs them
// all) and presents the available languages. Each language is a separate
// installed `graded` element type with its own CodeMirror 6 editor + grader.

(function () {
  "use strict";

  // Languages this suite ships, in dependency order. Each maps to an
  // `editor-<lang>` plugin; the runtime is the zero-import wasm engine that
  // powers both live eval and the host grader.
  var LANGUAGES = [
    { name: "JavaScript", runtime: "Boa (WebAssembly)" },
    { name: "TypeScript", runtime: "Boa + sucrase (type strip)" },
    { name: "C++", runtime: "JSCPP (interpreter)" },
    { name: "Python", runtime: "RustPython (WebAssembly)" },
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
      var card = el("div", "card");
      card.appendChild(el("div", "card-name", l.name));
      card.appendChild(el("div", "card-runtime", l.runtime));
      card.appendChild(el("div", "card-tag", "runs locally"));
      host.appendChild(card);
    });
  }

  function boot() {
    renderLanguages();
    if (typeof window.alex !== "undefined") {
      alex.onHost(function (msg) {
        if (msg && msg.type === "init") {
          try {
            alex.complete(1);
          } catch (e) {}
        }
      });
      void alex.ready([]);
    }
  }

  boot();
})();
