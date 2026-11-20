// Alexandria package wrapper.
//
// The player loads this page (`<prefix>/_alexandria/wrapper.html`) in its
// package frame. It sits at the package origin, so a lesson in the nested
// frame finds the run-time API here by the standard's discovery algorithm
// and calls it synchronously. Run-time state lives here, in the vendored
// runtime; the host (the app page, another origin) hears about it only
// through the messages below.
//
// Host -> wrapper, accepted only from `window.parent`:
//   { source: 'alexandria-player', type: 'launch', standard, activityId, url,
//     title, runtime, commitIntervalMs }
//       Start one lesson session: build a fresh API, load `runtime` (the host's
//       initialisation data, `{ cmi: {...} }`), then open `url` in a new
//       lesson frame. `url` must lie under this launch's prefix.
//   { source: 'alexandria-player', type: 'end' }
//       End the current lesson session (answered with `ended`).
//
// Wrapper -> host:
//   { source: 'alexandria-wrapper', type: 'ready' }
//   { source: 'alexandria-wrapper', type: 'commit', activityId, sessionEnded, runtime }
//       The whole run-time state of the lesson, coalesced: at most one per
//       `commitIntervalMs`, and at once when the session ends, when this page
//       is hidden (app backgrounded) or unloaded.
//   { source: 'alexandria-wrapper', type: 'ended', activityId }
//       The session is over and its last commit has been sent.
//   { source: 'alexandria-wrapper', type: 'refused', reason }
//       A launch this wrapper will not run.
//
// Content can run script in this page (it is same-origin with the lesson), so
// nothing here is a security boundary: the host checks which lesson a commit
// is for and the bootstrap, content policy and launch prefix do the
// isolation. What this page guarantees is the run-time behaviour: a facade
// bound to one session, coalesced commits, and a session always finished.

(function () {
  'use strict';

  var FROM_PLAYER = 'alexandria-player';
  var FROM_WRAPPER = 'alexandria-wrapper';
  var DEFAULT_COMMIT_INTERVAL_MS = 2000;

  var host = window.parent;
  var hostOrigin = null;
  var here = location.href.split(/[?#]/)[0];
  var launchPrefix = here.slice(0, here.lastIndexOf('/_alexandria/') + 1);
  var session = null;

  function tell(message) {
    message.source = FROM_WRAPPER;
    try {
      host.postMessage(message, hostOrigin || '*');
    } catch (_) {}
  }

  // One entry per standard: the runtime script under `_alexandria/`, the
  // window property content looks for, and how to build a session's API and
  // facade. SCORM 2004 (ticket 10) adds its entry here.
  var RUNTIMES = {
    scorm12: {
      script: 'scorm12.min.js',
      global: 'Scorm12API',
      apiName: 'API',
      create: createScorm12,
    },
  };

  function createScorm12(Runtime, s) {
    var api = new Runtime({
      // Any string routes each commit to `httpService`; nothing is fetched.
      lmsCommitUrl: 'host:',
      httpService: {
        processHttpRequest: function (_url, payload, _immediate, _log, _notify, metadata) {
          received(s, payload, Boolean(metadata && metadata.isTerminateCommit));
          // Always true: a persistence failure is the host's to show, and
          // answering false would leave LMSFinish's session open.
          return { result: 'true', errorCode: 0 };
        },
        updateSettings: function () {},
      },
      logLevel: 5,
      // A lesson that never sets its status finishes as completed (then the
      // mastery score applies). The library marks the default status as set
      // by the lesson at LMSInitialize; undo that so the rule can run.
      autoCompleteLessonStatus: true,
    });
    api.on('LMSInitialize', function () {
      api.statusSetByModule = false;
    });

    // Calls after the session finished, or after the host ended it, are
    // refused here: the library would accept them and send a late commit.
    var NOT_INITIALISED = '301';
    var refusal = null;
    function live() {
      return !s.finished && !s.closed;
    }
    function refuse(value) {
      refusal = NOT_INITIALISED;
      return value;
    }
    function pass(value) {
      refusal = null;
      return value;
    }
    var facade = {
      LMSInitialize: function (parameter) {
        if (s.closed) return refuse('false');
        return pass(api.LMSInitialize(parameter));
      },
      LMSFinish: function (parameter) {
        if (!live()) return refuse('false');
        var result = api.LMSFinish(parameter);
        if (result === 'true') s.finished = true;
        return pass(result);
      },
      LMSGetValue: function (element) {
        if (!live()) return refuse('');
        return pass(api.LMSGetValue(element));
      },
      LMSSetValue: function (element, value) {
        if (!live()) return refuse('false');
        return pass(api.LMSSetValue(element, value));
      },
      LMSCommit: function (parameter) {
        if (!live()) return refuse('false');
        return pass(api.LMSCommit(parameter));
      },
      LMSGetLastError: function () {
        return refusal || api.LMSGetLastError();
      },
      LMSGetErrorString: function (code) {
        return api.LMSGetErrorString(code);
      },
      LMSGetDiagnostic: function (code) {
        return api.LMSGetDiagnostic(code);
      },
    };
    return {
      load: function (runtime) {
        api.loadFromJSON(runtime);
      },
      finish: function () {
        if (api.isInitialized()) api.LMSFinish('');
      },
      facade: facade,
    };
  }

  function received(s, payload, sessionEnded) {
    if (s.closed && !s.ending) return;
    s.pending = payload;
    if (sessionEnded) {
      s.pendingEnded = true;
      flush(s);
    } else if (!s.timer) {
      s.timer = setTimeout(function () {
        s.timer = null;
        flush(s);
      }, s.interval);
    }
  }

  function flush(s) {
    if (s.timer) {
      clearTimeout(s.timer);
      s.timer = null;
    }
    if (!s.pending) return;
    var runtime = s.pending;
    var ended = s.pendingEnded;
    s.pending = null;
    s.pendingEnded = false;
    tell({ type: 'commit', activityId: s.activityId, sessionEnded: ended, runtime: runtime });
  }

  // Ends the current session: the lesson frame goes (its own unload handlers
  // run and may finish the session), the wrapper finishes it if the lesson
  // did not, and the last state is sent before `ended`.
  function endSession() {
    var s = session;
    if (!s) return;
    session = null;
    s.ending = true;
    if (s.frame.parentNode) s.frame.parentNode.removeChild(s.frame);
    if (!s.finished) {
      s.finished = true;
      try {
        s.runtime.finish();
      } catch (_) {}
    }
    flush(s);
    s.closed = true;
    s.ending = false;
    try {
      if (window[s.apiName] === s.runtime.facade) delete window[s.apiName];
    } catch (_) {}
    tell({ type: 'ended', activityId: s.activityId });
  }

  var loading = {};
  function withRuntime(entry, then) {
    if (window[entry.global]) return then(window[entry.global]);
    if (!loading[entry.script]) {
      loading[entry.script] = [];
      var script = document.createElement('script');
      script.src = entry.script;
      script.onload = function () {
        var waiting = loading[entry.script];
        loading[entry.script] = null;
        waiting.forEach(function (next) {
          next(window[entry.global]);
        });
      };
      script.onerror = function () {
        tell({ type: 'refused', reason: 'runtime-unavailable' });
      };
      document.head.appendChild(script);
    }
    loading[entry.script].push(then);
  }

  function launch(message) {
    endSession();
    var entry = RUNTIMES[message.standard];
    if (!entry) return tell({ type: 'refused', reason: 'unsupported-standard' });
    var url = typeof message.url === 'string' ? message.url : '';
    if (url.indexOf(launchPrefix) !== 0 || url.indexOf(launchPrefix + '_alexandria/') === 0) {
      return tell({ type: 'refused', reason: 'outside-launch' });
    }
    withRuntime(entry, function (Runtime) {
      var s = {
        activityId: String(message.activityId),
        apiName: entry.apiName,
        interval: Number(message.commitIntervalMs) > 0 ? Number(message.commitIntervalMs) : DEFAULT_COMMIT_INTERVAL_MS,
        pending: null,
        pendingEnded: false,
        timer: null,
        finished: false,
        closed: false,
        ending: false,
        frame: null,
        runtime: null,
      };
      s.runtime = entry.create(Runtime, s);
      // Prior state goes in before the lesson exists: the runtime refuses it
      // after initialise, and most lessons initialise as soon as they load.
      s.runtime.load(message.runtime);
      window[entry.apiName] = s.runtime.facade;
      var frame = document.createElement('iframe');
      frame.id = 'lesson';
      frame.title = typeof message.title === 'string' ? message.title : '';
      frame.src = url;
      s.frame = frame;
      session = s;
      document.body.appendChild(frame);
    });
  }

  window.addEventListener('message', function (event) {
    if (event.source !== host) return;
    var data = event.data;
    if (!data || data.source !== FROM_PLAYER) return;
    hostOrigin = event.origin && event.origin !== 'null' ? event.origin : hostOrigin;
    if (data.type === 'launch') launch(data);
    else if (data.type === 'end') {
      if (session) endSession();
      else tell({ type: 'ended', activityId: null });
    }
  });

  // The app went to the background: hand over what is pending now.
  document.addEventListener('visibilitychange', function () {
    if (document.visibilityState === 'hidden' && session) flush(session);
  });

  // The package frame is going away (the player closed or navigated it).
  window.addEventListener('pagehide', endSession);

  tell({ type: 'ready' });
})();
