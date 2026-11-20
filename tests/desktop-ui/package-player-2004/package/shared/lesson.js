// Shared by the fixture's lessons. It finds the SCORM 2004 API with the
// discovery algorithm published with the standard, starts a session, writes
// what the lesson was given into `#seen` so a test can read it, and wires the
// lesson's buttons. Like most authoring tools' output, it terminates the
// session itself when the page goes away.
var lesson = (function () {
  'use strict';

  function scan(win) {
    var tries = 0;
    while (win.API_1484_11 == null && win.parent != null && win.parent != win) {
      tries++;
      if (tries > 500) return null;
      win = win.parent;
    }
    return win.API_1484_11;
  }

  var api = scan(window);
  if (api == null && window.opener != null) api = scan(window.opener);
  var finished = false;

  function show(name, value) {
    var seen = document.getElementById('seen');
    var values = JSON.parse(seen.getAttribute('data-values') || '{}');
    values[name] = value;
    seen.setAttribute('data-values', JSON.stringify(values));
    seen.textContent = JSON.stringify(values, null, 1);
  }

  function set(element, value) {
    show('set ' + element, api.SetValue(element, value));
  }

  function commit() {
    show('committed', api.Commit(''));
  }

  function terminate() {
    if (finished) return;
    finished = true;
    show('terminated', api.Terminate(''));
  }

  var READ = [
    'cmi.entry', 'cmi.learner_id', 'cmi.learner_name', 'cmi.location', 'cmi.suspend_data',
    'cmi.launch_data', 'cmi.scaled_passing_score', 'cmi.completion_status', 'cmi.mode', 'cmi.credit',
  ];

  var actions = {
    pass: function () {
      set('cmi.score.scaled', '0.9');
      set('cmi.success_status', 'passed');
      set('cmi.completion_status', 'completed');
      commit();
    },
    complete: function () {
      set('cmi.completion_status', 'completed');
      set('cmi.success_status', 'passed');
      commit();
    },
    bookmark: function () {
      set('cmi.location', document.body.getAttribute('data-lesson') + '-2');
      set('cmi.suspend_data', 'seen=1,2');
      set('cmi.completion_status', 'incomplete');
      commit();
    },
    next: function () {
      set('adl.nav.request', 'continue');
      terminate();
    },
    finish: function () {
      terminate();
    },
  };

  window.addEventListener('DOMContentLoaded', function () {
    show('initialised', api.Initialize(''));
    READ.forEach(function (element) {
      show(element, api.GetValue(element));
    });
    document.getElementById('seen').setAttribute('data-ready', '');
    Object.keys(actions).forEach(function (name) {
      var button = document.getElementById(name);
      if (button) button.addEventListener('click', actions[name]);
    });
  });
  window.addEventListener('pagehide', terminate);

  return { api: api, show: show, set: set };
})();
