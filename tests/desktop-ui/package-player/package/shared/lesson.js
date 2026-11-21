// Shared by the fixture's lessons, the way authoring tools ship one runtime
// script with every lesson. It finds the SCORM 1.2 API with the discovery
// algorithm published with the standard, starts a session, and writes what
// the lesson was given into `#seen` so a test can read it.
var lesson = (function () {
  'use strict';

  function findAPI(win) {
    var tries = 0;
    while (win.API == null && win.parent != null && win.parent != win) {
      tries++;
      if (tries > 7) return null;
      win = win.parent;
    }
    return win.API;
  }

  var api = findAPI(window);
  if (api == null && window.opener != null) api = findAPI(window.opener);

  var finished = false;

  function show(name, value) {
    var seen = document.getElementById('seen');
    var values = JSON.parse(seen.getAttribute('data-values') || '{}');
    values[name] = value;
    seen.setAttribute('data-values', JSON.stringify(values));
    seen.textContent = JSON.stringify(values, null, 1);
  }

  function get(element) {
    return api.LMSGetValue(element);
  }

  function set(element, value) {
    return api.LMSSetValue(element, value);
  }

  function start(elements) {
    show('initialised', api.LMSInitialize(''));
    elements.forEach(function (element) {
      show(element, get(element));
    });
    document.getElementById('seen').setAttribute('data-ready', '');
  }

  function commit() {
    show('committed', api.LMSCommit(''));
  }

  function finish() {
    if (finished) return;
    finished = true;
    show('finished', api.LMSFinish(''));
  }

  return { api: api, get: get, set: set, start: start, commit: commit, finish: finish, show: show };
})();
