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
// SCORM 2004 runs one API for the whole package under its sequencing engine
// and adds messages of its own; see the section at the end of this file.
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
  // facade. A `sequenced` standard has one API for the whole package and its
  // own lifecycle (SCORM 2004, at the end of this file).
  var RUNTIMES = {
    scorm12: {
      script: 'scorm12.min.js',
      global: 'Scorm12API',
      apiName: 'API',
      create: createScorm12,
    },
    scorm2004: {
      script: 'scorm2004.min.js',
      global: 'Scorm2004API',
      apiName: 'API_1484_11',
      sequenced: true,
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

  // A lesson address the wrapper will open: under this launch's prefix and
  // not one of the reserved files.
  function insideLaunch(url) {
    return typeof url === 'string' && url.indexOf(launchPrefix) === 0 && url.indexOf(launchPrefix + '_alexandria/') !== 0;
  }

  function launch(message) {
    var entry = RUNTIMES[message.standard];
    if (entry && entry.sequenced) return launchDelivered(message);
    endSession();
    if (!entry) return tell({ type: 'refused', reason: 'unsupported-standard' });
    var url = message.url;
    if (!insideLaunch(url)) {
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
    else if (data.type === 'sequence') sequence(data);
    else if (data.type === 'navigate') navigate(data);
    else if (data.type === 'end') {
      if (pkg) closePackage();
      else if (session) endSession();
      else tell({ type: 'ended', activityId: null });
    }
  });

  // The app went to the background: hand over what is pending now.
  document.addEventListener('visibilitychange', function () {
    if (document.visibilityState !== 'hidden') return;
    if (session) flush(session);
    if (pkg && pkg.lesson) flush(pkg.lesson);
  });

  // The package frame is going away (the player closed or navigated it).
  window.addEventListener('pagehide', function () {
    if (pkg) closePackage();
    else endSession();
  });

  // ---- SCORM 2004 ---------------------------------------------------------
  //
  // One API instance serves the whole package. The sequencing engine inside
  // it keeps the activity tree, tracking and global objectives across lessons
  // and decides what is delivered; `reset()` starts each delivered lesson's
  // own run-time session, and the facade on `window.API_1484_11` is bound to
  // that one delivery, so a late call from a lesson that has gone is refused.
  //
  // Host -> wrapper:
  //   { type: 'sequence', standard: 'scorm2004', settings: { activityTree },
  //     snapshot, commitIntervalMs }
  //       Build the package's API, restore `snapshot` (the engine's state from
  //       the last launch, or null) and start: resume all if the snapshot
  //       allows it, else start.
  //   { type: 'navigate', request, target }
  //       A request from the player's controls: `continue`, `previous`,
  //       `choice` (with `target`), `suspendAll`, `exitAll`, `resumeAll`. A
  //       running lesson is ended first (its frame goes, so its own handlers
  //       can terminate) and the request travels with its termination.
  //   { type: 'launch', standard: 'scorm2004', activityId, url, title, runtime }
  //       Open the activity the engine delivered, with the host's
  //       initialisation data. Any other activity is refused.
  //   { type: 'end' }
  //       The player is closing: suspend all (unless the session is over),
  //       report, then `closed`.
  //
  // Wrapper -> host, after every navigation (a lesson's own termination, a
  // host request, the start):
  //   { type: 'sequencing-state', snapshot, root: { completion_status } }
  //   { type: 'navigation', current, continue, previous, hidden, active, tree }
  //       Controls the player draws: enabled from the engine's preview of the
  //       request, hidden as the delivered activity's hideLMSUI says. `tree`
  //       is the activity tree as plain data for the table of contents.
  //   then one of:
  //   { type: 'delivered', activityId, resumed }
  //   { type: 'undelivered' }                 nothing delivered: the learner chooses
  //   { type: 'session-ended', reason }       suspended, exited or finished
  // plus `commit` and `ended` for each lesson session as for SCORM 1.2, and
  // `closed` once `end` has been handled.

  var pkg = null;
  var NAVIGATION_REQUESTS = { continue: 1, previous: 1, choice: 1, suspendAll: 1, exitAll: 1, resumeAll: 1, start: 1 };

  function createPackage(Runtime, settings, interval) {
    var p = {
      api: null,
      settings: settings,
      interval: interval,
      lesson: null,
      delivered: null,
      expecting: null,
      over: null,
      validity: null,
      controlsTimer: null,
      closing: false,
    };
    p.api = new Runtime({
      lmsCommitUrl: 'host:',
      httpService: {
        processHttpRequest: function (_url, payload, _immediate, _log, _notify, metadata) {
          if (p.lesson) received(p.lesson, payload, Boolean(metadata && metadata.isTerminateCommit));
          return { result: 'true', errorCode: 0 };
        },
        updateSettings: function () {},
      },
      logLevel: 5,
      sequencing: {
        activityTree: settings.activityTree,
        logLevel: 'error',
        eventListeners: {
          onActivityDelivery: function (activity) {
            p.delivered = { id: String(activity.id), resumed: Boolean(activity.deliveryWasResumed) };
            p.over = null;
          },
          onNavigationValidityUpdate: function (validity) {
            p.validity = validity;
          },
          onSequencingSessionEnd: function (event) {
            p.over = (event && event.reason) || 'ended';
          },
        },
      },
    });
    measureAttemptDurations(p.api);
    return p;
  }

  // The runtime's activities read `attemptAbsoluteDuration` as the attempt's
  // duration limit, not its duration, so the limit conditions check finds
  // every limit already reached and nothing with a duration limit is ever
  // delivered (test suite CM-01). Point it at the measured duration, which is
  // what the check, the new-attempt reset and the state restore mean.
  function measureAttemptDurations(api) {
    var root = null;
    try {
      root = api.getSequencingState().rootActivity;
    } catch (_) {}
    (function walk(activity) {
      if (!activity) return;
      // Only where a limit is set: with the accessor on every activity the
      // engine's continue look-ahead stops answering (the player fixture).
      if (activity.attemptAbsoluteDurationLimit) try {
        Object.defineProperty(activity, 'attemptAbsoluteDuration', {
          configurable: true,
          get: function () {
            return this.attemptAbsoluteDurationValue;
          },
          set: function (value) {
            try {
              this.attemptAbsoluteDurationValue = value;
            } catch (_) {}
          },
        });
      } catch (_) {}
      (activity.children || []).forEach(walk);
    })(root);
  }

  function sequence(message) {
    var entry = RUNTIMES[message.standard];
    if (!entry || !entry.sequenced) return tell({ type: 'refused', reason: 'unsupported-standard' });
    var settings = message.settings && typeof message.settings === 'object' ? message.settings : {};
    var interval = Number(message.commitIntervalMs) > 0 ? Number(message.commitIntervalMs) : DEFAULT_COMMIT_INTERVAL_MS;
    withRuntime(entry, function (Runtime) {
      pkg = restore(Runtime, settings, interval, typeof message.snapshot === 'string' ? message.snapshot : '');
      pkg.Runtime = Runtime;
      report();
    });
  }

  // A package API with the engine's state restored, started or resumed.
  // A snapshot taken without suspend all accepts neither start nor resume
  // all; suspending it first recovers the attempt.
  function restore(Runtime, settings, interval, snapshot) {
    var p = createPackage(Runtime, settings, interval);
    var api = p.api;
    var restored = false;
    if (snapshot) {
      try {
        restored = api.deserializeSequencingState(snapshot) === true;
        measureAttemptDurations(api);
      } catch (_) {}
    }
    var resumed = restored && (api.processNavigationRequest('resumeAll')
      || (api.processNavigationRequest('suspendAll') && api.processNavigationRequest('resumeAll')));
    if (!resumed) {
      p.over = null;
      api.processNavigationRequest('start');
    }
    return p;
  }

  function launchDelivered(message) {
    if (!pkg || pkg.expecting === null || String(message.activityId) !== pkg.expecting) {
      return tell({ type: 'refused', reason: 'not-delivered' });
    }
    if (!insideLaunch(message.url)) return tell({ type: 'refused', reason: 'outside-launch' });
    pkg.expecting = null;
    var api = pkg.api;
    api.reset(undefined, { preserveListeners: true, resetTotalTime: true });
    forgetPreviousLesson(api);
    // Prior state goes in before the lesson exists: the runtime refuses it
    // after initialise.
    try {
      api.loadFromJSON(message.runtime);
    } catch (_) {}
    var s = {
      activityId: String(message.activityId),
      apiName: RUNTIMES.scorm2004.apiName,
      interval: pkg.interval,
      pending: null,
      pendingEnded: false,
      timer: null,
      finished: false,
      closed: false,
      ending: false,
      frame: null,
      runtime: null,
    };
    s.runtime = { facade: deliveryFacade(api, s) };
    pkg.lesson = s;
    window.API_1484_11 = s.runtime.facade;
    var frame = document.createElement('iframe');
    frame.id = 'lesson';
    frame.title = typeof message.title === 'string' ? message.title : '';
    frame.src = message.url;
    s.frame = frame;
    document.body.appendChild(frame);
    tell(navigationState());
  }

  // `reset()` keeps the previous lesson's launch values and learner comments,
  // so a lesson the manifest gives none would read another lesson's (test
  // suite DMB: a lesson without a passing score read the one before's). Put
  // the standard's initial values back; the host's values follow. Written
  // directly, not through SetValue, so an unset value still reads as unset.
  var LAUNCH_DEFAULTS = { scaled_passing_score: '', launch_data: '', max_time_allowed: '', time_limit_action: 'continue,no message' };
  function forgetPreviousLesson(api) {
    var cmi = api.cmi;
    Object.keys(LAUNCH_DEFAULTS).forEach(function (name) {
      try {
        cmi[name] = LAUNCH_DEFAULTS[name];
      } catch (_) {}
    });
    try {
      cmi.comments_from_learner.reset(true);
      cmi.comments_from_lms.reset(true);
    } catch (_) {}
  }

  // The eight functions, bound to one delivered lesson. After that lesson
  // terminated, or once the wrapper ended it, calls are refused with the
  // standard's after-termination codes instead of reaching the next lesson's
  // session.
  function deliveryFacade(api, s) {
    var refusal = null;
    function live() {
      return !s.finished && !s.closed;
    }
    function refuse(value, code) {
      refusal = code;
      return value;
    }
    function pass(value) {
      refusal = null;
      return value;
    }
    return {
      Initialize: function (parameter) {
        if (!live()) return refuse('false', '104');
        var result = api.Initialize(parameter);
        controlsSoon();
        return pass(result);
      },
      Terminate: function (parameter) {
        if (!live()) return refuse('false', '113');
        var result = api.Terminate(parameter);
        if (result === 'true') {
          s.finished = true;
          // The engine has navigated inside Terminate; the next lesson opens
          // once the lesson's own call has returned.
          setTimeout(function () {
            if (pkg && pkg.lesson === s && !s.closed) {
              endDelivery(s, null);
              report();
            }
          }, 0);
        }
        return pass(result);
      },
      GetValue: function (element) {
        if (!live()) return refuse('', '123');
        if (isContainer(element) && api.isInitialized()) return refuse('', '401');
        var value = api.GetValue(element);
        // The runtime leaves the previous call's error code in place for
        // these two; a successful read sets 0.
        if ((element === 'cmi.completion_status' || element === 'cmi.success_status') && api.isInitialized()) {
          api.lastErrorCode = '0';
        }
        return pass(value);
      },
      SetValue: function (element, value) {
        if (!live()) return refuse('false', '133');
        // The runtime would replace a part of its data model with the value.
        if (isContainer(element) && api.isInitialized()) return refuse('false', '401');
        var result = api.SetValue(element, value);
        controlsSoon();
        return pass(result);
      },
      Commit: function (parameter) {
        if (!live()) return refuse('false', '143');
        return pass(api.Commit(parameter));
      },
      GetLastError: function () {
        return refusal || api.GetLastError();
      },
      GetErrorString: function (code) {
        return api.GetErrorString(code);
      },
      GetDiagnostic: function (code) {
        return api.GetDiagnostic(code);
      },
    };
  }

  // A name for a group of elements (`cmi`, `cmi.objectives`, `adl.data`, an
  // entry of a list) names no element: 401 (test suite DMI). The runtime
  // reads and replaces its own object for it instead.
  var CONTAINER = new RegExp('^(?:cmi|adl|adl\\.(?:nav|nav\\.request_valid|data(?:\\.\\d+)?)' +
    '|cmi\\.(?:score|learner_preference|comments_from_learner|comments_from_lms|objectives|interactions)' +
    '|cmi\\.(?:comments_from_learner|comments_from_lms)\\.\\d+' +
    '|cmi\\.objectives\\.\\d+(?:\\.score)?' +
    '|cmi\\.interactions\\.\\d+(?:\\.(?:objectives|correct_responses)(?:\\.\\d+)?)?)$');
  function isContainer(element) {
    return typeof element === 'string' && CONTAINER.test(element);
  }

  // Ends a delivered lesson's session: `request` (a navigation request, or
  // null) is set first so it travels with the termination, the frame goes
  // (its own handlers may terminate), the wrapper terminates the session if
  // the lesson did not, and the last state is sent before `ended`. Returns
  // whether a termination carried `request`.
  function endDelivery(s, request) {
    var api = pkg.api;
    var wasLive = !s.finished && api.isInitialized();
    s.ending = true;
    if (wasLive && request) {
      try {
        api.SetValue('adl.nav.request', request);
      } catch (_) {}
    }
    if (s.frame.parentNode) s.frame.parentNode.removeChild(s.frame);
    if (!s.finished) {
      s.finished = true;
      try {
        if (api.isInitialized()) api.Terminate('');
      } catch (_) {}
    }
    flush(s);
    s.closed = true;
    s.ending = false;
    pkg.lesson = null;
    try {
      if (window.API_1484_11 === s.runtime.facade) delete window.API_1484_11;
    } catch (_) {}
    tell({ type: 'ended', activityId: s.activityId });
    return wasLive && Boolean(request);
  }

  function navigate(message) {
    if (!pkg || pkg.closing) return;
    var request = String(message.request || '');
    if (!NAVIGATION_REQUESTS[request]) return;
    var target = typeof message.target === 'string' ? message.target : '';
    if (request === 'choice' && !target) return;
    // After suspend all or exit all the engine has ended its session; the
    // next request runs on a fresh instance restored from its own snapshot.
    if (pkg.over && (request === 'resumeAll' || request === 'start' || request === 'choice')) {
      var snapshot = pkg.api.serializeSequencingState();
      pkg = restoreFor(request, target, snapshot);
      return report();
    }
    pkg.delivered = null;
    var s = pkg.lesson;
    var withTermination = request === 'choice' ? '{target=' + target + '}choice' : request;
    var carried = s ? endDelivery(s, withTermination) : false;
    if (!carried) pkg.api.processNavigationRequest(request, target || undefined);
    report();
  }

  function restoreFor(request, target, snapshot) {
    var p = createPackage(pkg.Runtime, pkg.settings, pkg.interval);
    p.Runtime = pkg.Runtime;
    var api = p.api;
    try {
      api.deserializeSequencingState(snapshot);
      measureAttemptDurations(api);
    } catch (_) {}
    if (request === 'choice') api.processNavigationRequest('choice', target);
    else if (!api.processNavigationRequest(request)) api.processNavigationRequest('start');
    return p;
  }

  function closePackage() {
    if (!pkg || pkg.closing) return;
    pkg.closing = true;
    pkg.delivered = null;
    var api = pkg.api;
    var s = pkg.lesson;
    var carried = s ? endDelivery(s, pkg.over ? null : 'suspendAll') : false;
    if (!carried && !pkg.over && api.getSequencingState().currentActivity) {
      api.processNavigationRequest('suspendAll');
    }
    sendState();
    tell({ type: 'closed' });
  }

  function sendState() {
    var api = pkg.api;
    var root = api.getSequencingState().rootActivity;
    var tracking = root ? api.getActivityTrackingData(root.id) : null;
    var snapshot = api.serializeSequencingState();
    tell({
      type: 'sequencing-state',
      snapshot: typeof snapshot === 'string' ? snapshot : JSON.stringify(snapshot),
      root: { completion_status: tracking ? String(tracking.completionStatus) : 'unknown' },
    });
  }

  function report() {
    sendState();
    tell(navigationState());
    if (pkg.delivered) {
      pkg.expecting = pkg.delivered.id;
      tell({ type: 'delivered', activityId: pkg.delivered.id, resumed: pkg.delivered.resumed });
      pkg.delivered = null;
    } else if (pkg.over) {
      tell({ type: 'session-ended', reason: String(pkg.over) });
    } else {
      tell({ type: 'undelivered' });
    }
  }

  function controlsSoon() {
    if (!pkg || pkg.controlsTimer) return;
    pkg.controlsTimer = setTimeout(function () {
      if (!pkg) return;
      pkg.controlsTimer = null;
      tell(navigationState());
    }, 50);
  }

  // Continue and previous come from the engine's preview while a lesson is
  // running (the runtime's own validity flags only say a request may be
  // issued); otherwise, or when the preview cannot tell, from those flags.
  function navigationState() {
    var api = pkg.api;
    var validity = pkg.validity || {};
    var state = api.getSequencingState();
    var current = state.currentActivity;
    var running = pkg.lesson && !pkg.lesson.finished && api.isInitialized();
    function allowed(request) {
      if (pkg.over) return false;
      if (running) {
        var preview = api.previewNavigationRequest(request);
        if (preview.outcome === 'allowed') return true;
        if (preview.outcome === 'blocked') return false;
      }
      return validity[request] === true;
    }
    var choice = current ? validity.choice || {} : choicesFromRoot(api, state.rootActivity);
    return {
      type: 'navigation',
      current: current ? String(current.id) : null,
      continue: allowed('continue'),
      previous: allowed('previous'),
      hidden: (validity.hideLmsUi || []).map(String),
      active: !pkg.over && Boolean(current),
      tree: state.rootActivity ? plain(state.rootActivity, choice, hiddenFromChoice(api)) : null,
    };
  }

  // Whether an activity's precondition rules hide it from choice now. The
  // runtime evaluates such a rule only when a choice is made, so ask its rule
  // engine (semi-internal; recheck on upgrade). A hidden activity is left out
  // of the table of contents (test suite CM-07c, MS-07).
  function hiddenFromChoice(api) {
    var engine = null;
    try {
      engine = api.getSequencingService().sequencingProcess.ruleEngine;
    } catch (_) {}
    return function (activity) {
      if (activity.isHiddenFromChoice === true) return true;
      if (!engine) return false;
      try {
        var rules = ((activity.sequencingRules && activity.sequencingRules.preConditionRules) || []).filter(function (rule) {
          return rule.action === 'hiddenFromChoice';
        });
        return rules.length > 0 && engine.checkSequencingRules(activity, rules) === 'hiddenFromChoice';
      } catch (_) {
        return false;
      }
    };
  }

  // With no current activity (start delivered nothing) the runtime reports
  // no choices at all: its look-ahead from the root is an empty stub. Ask the
  // engine's own choice validation for each activity instead.
  function choicesFromRoot(api, root) {
    var choice = {};
    var process = null;
    try {
      process = api.getSequencingService().sequencingProcess;
    } catch (_) {}
    (function walk(activity) {
      if (!activity) return;
      var valid = true;
      try {
        if (process) valid = process.validateNavigationRequest('choice', activity.id, null).valid === true;
      } catch (_) {}
      choice[activity.id] = valid ? 'true' : 'false';
      (activity.children || []).forEach(walk);
    })(root);
    return choice;
  }

  // The engine's activity graph is live and cyclic; the player gets plain
  // data.
  function plain(activity, choice, hidden) {
    return {
      id: String(activity.id),
      title: String(activity.title || ''),
      visible: activity.isVisible !== false && !hidden(activity),
      completion: String(activity.completionStatus || 'unknown'),
      choosable: choice[activity.id] === 'true' || choice[activity.id] === true,
      children: (activity.children || [])
        .filter(function (child) {
          return child.isAvailable !== false;
        })
        .map(function (child) {
          return plain(child, choice, hidden);
        }),
    };
  }

  tell({ type: 'ready' });
})();
