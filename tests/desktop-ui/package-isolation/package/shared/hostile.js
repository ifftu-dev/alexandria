// Shared by the hostile lessons. Each lesson tries one kind of escape; this
// script finds the SCORM 1.2 API the ordinary way (so the lesson is a real
// SCO), writes what the lesson found into `#seen` for the test to read, and
// knows every way content can get hold of another window at the package
// origin, so an escape through a window the bootstrap never saw is tried too.
var hostile = (function () {
  'use strict';

  function findAPI(win) {
    var tries = 0;
    while (win.API == null && win.parent != null && win.parent != win) {
      if (++tries > 7) return null;
      win = win.parent;
    }
    return win.API;
  }

  var api = findAPI(window);
  if (api) api.LMSInitialize('');

  function show(name, value) {
    var seen = document.getElementById('seen');
    var values = JSON.parse(seen.getAttribute('data-values') || '{}');
    values[name] = value;
    seen.setAttribute('data-values', JSON.stringify(values));
  }

  function ready() {
    document.getElementById('seen').setAttribute('data-ready', '');
  }

  function done() {
    document.getElementById('seen').setAttribute('data-done', '');
  }

  // Windows reached from inside another document report back through here.
  var visitor = null;
  var early = [];
  window.__hostileVisit = function (win, how) {
    if (visitor) visitor(win, how);
    else early.push([win, how]);
  };

  function attempt(visit, win, how) {
    try {
      visit(win, how);
    } catch (error) {
      show(how, 'threw ' + (error && error.name));
    }
  }

  function frame(attributes, parent) {
    var element = document.createElement('iframe');
    Object.keys(attributes || {}).forEach(function (name) {
      element.setAttribute(name, attributes[name]);
    });
    (parent || document.body).appendChild(element);
    return element;
  }

  var REPORT_BACK = '<script>try { parent.__hostileVisit(window, NAME) } catch (e) {}<\/script>';
  function reportBack(how) {
    return REPORT_BACK.replace('NAME', JSON.stringify(how).replace(/"/g, '&quot;'));
  }
  function reportBackRaw(how) {
    return REPORT_BACK.replace('NAME', JSON.stringify(how));
  }

  // Runs one way of reaching a window; a way that throws is reported and
  // the rest still run.
  function step(how, work) {
    try {
      work();
    } catch (error) {
      show(how, 'threw ' + (error && error.name));
    }
  }

  // Calls `visit(win, how)` for every window content can reach, now or as it
  // appears, then `finished()` once the slow ones have had their chance.
  function everyWindow(visit, finished) {
    setTimeout(finished, 2000);
    visitor = function (win, how) { attempt(visit, win, how); };
    early.forEach(function (entry) { visitor(entry[0], entry[1]); });

    attempt(visit, window, 'own window');
    attempt(visit, parent, 'wrapper window');
    attempt(visit, top, 'app window');

    // A new frame, through the frame list before anything else can run.
    frame();
    attempt(visit, window.frames[window.frames.length - 1], 'new frame through frames[i]');

    // A new frame through its element.
    var element = frame();
    attempt(visit, element.contentWindow, 'new frame through contentWindow');

    // A named frame, through named access on the window.
    frame({ name: 'hostile-named' });
    attempt(visit, window['hostile-named'], 'new frame through its name');

    // A frame inside a new frame.
    var outer = window.frames[window.frames.length - 1];
    try {
      outer.document.body.appendChild(outer.document.createElement('iframe'));
      attempt(visit, outer.frames[0], 'frame inside a new frame');
    } catch (error) {
      show('frame inside a new frame', 'threw ' + error.name);
    }

    // A document written into a new frame.
    frame();
    try {
      var written = window.frames[window.frames.length - 1].document;
      written.open();
      written.write(reportBackRaw('written frame'));
      written.close();
    } catch (error) {
      show('written frame', 'threw ' + error.name);
    }

    // Frames whose documents are not served at all.
    step('srcdoc attribute', function () { frame({ srcdoc: reportBackRaw('srcdoc attribute') }); });
    var property = document.createElement('iframe');
    try { property.srcdoc = reportBackRaw('srcdoc property'); } catch (error) { show('srcdoc property', 'threw ' + error.name); }
    document.body.appendChild(property);
    var holder = document.createElement('div');
    try { holder.innerHTML = '<iframe srcdoc="' + reportBack('srcdoc in parsed markup') + '"></iframe>'; } catch (error) { show('srcdoc in parsed markup', 'threw ' + error.name); }
    document.body.appendChild(holder);
    try {
      document.body.insertAdjacentHTML('beforeend', '<div><iframe srcdoc="' + reportBack('srcdoc in inserted markup') + '"></iframe></div>');
    } catch (error) {
      show('srcdoc in inserted markup', 'threw ' + error.name);
    }
    step('javascript address', function () { frame({ src: 'javascript:"' + reportBackRaw('javascript address').replace(/"/g, '\\"') + '"' }); });
    var late = frame();
    try { late.setAttribute('srcdoc', reportBackRaw('srcdoc set after insertion')); } catch (error) { show('srcdoc set after insertion', 'threw ' + error.name); }

    // Frames inside a shadow tree, which events from the frame never leave.
    try {
      var host = document.createElement('div');
      document.body.appendChild(host);
      var shadow = host.attachShadow({ mode: 'closed' });
      var hidden = document.createElement('iframe');
      shadow.appendChild(hidden);
      attempt(visit, hidden.contentWindow, 'frame in a shadow tree');
      var hiddenSrcdoc = document.createElement('iframe');
      hiddenSrcdoc.setAttribute('srcdoc', reportBackRaw('srcdoc in a shadow tree'));
      shadow.appendChild(hiddenSrcdoc);
    } catch (error) {
      show('frame in a shadow tree', 'threw ' + error.name);
    }

    // A frame sent to a fresh blank document, watched every turn.
    var renewed = frame();
    var renewedWindow = window.frames[window.frames.length - 1];
    var first = null;
    try { first = renewedWindow.document; } catch (_) {}
    var polls = 0;
    var poll = setInterval(function () {
      polls++;
      var current = null;
      try { current = renewedWindow.document; } catch (_) {}
      if (current && current !== first) {
        clearInterval(poll);
        attempt(visit, renewedWindow, 'frame sent to a new blank page');
      } else if (polls > 200) {
        clearInterval(poll);
      }
    }, 0);
    try { renewed.contentWindow.location.replace('about:blank'); } catch (error) { show('frame sent to a new blank page', 'threw ' + error.name); }

    // A page that replaces itself with a document made from a javascript:
    // address. It reports back through its parent, this lesson.
    frame({ src: 'shared/self-replacing.html' });
  }

  return { api: api, show: show, ready: ready, done: done, everyWindow: everyWindow, frame: frame };
})();
