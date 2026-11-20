// A small cmi5 assignable unit, shared by the fixture's units the way an
// authoring tool ships one client with every unit. It reads the launch
// parameters, takes its token from the fetch URL, reads LMS.LaunchData and
// the learner preferences, sends `initialized`, and offers buttons for the
// rest of a session. What it was given and how each request went is written
// into `#seen[data-values]` so a test can read it.
var unit = (function () {
  'use strict';

  var params = new URLSearchParams(location.search);
  var endpoint = params.get('endpoint');
  var fetchUrl = params.get('fetch');
  var actor = JSON.parse(params.get('actor') || 'null');
  var registration = params.get('registration');
  var activityId = params.get('activityId');
  var token = null;
  var launchData = null;
  var startedAt = Date.now();

  var seen = {
    endpoint: endpoint,
    registration: registration,
    activityId: activityId,
    actor: actor,
    paramA: params.get('paramA'),
    sent: [],
  };

  function show() {
    var element = document.getElementById('seen');
    element.setAttribute('data-values', JSON.stringify(seen));
    element.textContent = JSON.stringify(seen, null, 1);
  }

  function enc(value) {
    return encodeURIComponent(value);
  }

  function uuid() {
    if (window.crypto && crypto.randomUUID) return crypto.randomUUID();
    var bytes = new Uint8Array(16);
    crypto.getRandomValues(bytes);
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    var hex = Array.prototype.map.call(bytes, function (b) { return ('0' + b.toString(16)).slice(-2); }).join('');
    return hex.slice(0, 8) + '-' + hex.slice(8, 12) + '-' + hex.slice(12, 16) + '-' + hex.slice(16, 20) + '-' + hex.slice(20);
  }

  function xapi(method, path, body) {
    return fetch(endpoint + path, {
      method: method,
      headers: {
        Authorization: 'Basic ' + token,
        'X-Experience-API-Version': '1.0.3',
        'Content-Type': 'application/json',
      },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
  }

  function duration() {
    return 'PT' + ((Date.now() - startedAt) / 1000).toFixed(2) + 'S';
  }

  // A cmi5 defined statement on the context template.
  function statement(verb, result) {
    var template = (launchData && launchData.contextTemplate) || {};
    var activities = JSON.parse(JSON.stringify(template.contextActivities || {}));
    var category = [{ id: 'https://w3id.org/xapi/cmi5/context/categories/cmi5' }];
    if (result && (result.success !== undefined || result.completion !== undefined)) {
      category.push({ id: 'https://w3id.org/xapi/cmi5/context/categories/moveon' });
    }
    activities.category = (activities.category || []).concat(category);
    var context = {
      registration: registration,
      contextActivities: activities,
      extensions: template.extensions || {},
    };
    var out = {
      id: uuid(),
      actor: actor,
      verb: { id: 'http://adlnet.gov/expapi/verbs/' + verb, display: { 'en-US': verb } },
      object: { objectType: 'Activity', id: activityId },
      context: context,
      timestamp: new Date().toISOString(),
    };
    if (result) out.result = result;
    return out;
  }

  function send(verb, result) {
    return xapi('POST', 'statements', statement(verb, result)).then(function (response) {
      seen.sent.push({ verb: verb, status: response.status });
      show();
      return response.status;
    });
  }

  function start() {
    show();
    return fetch(fetchUrl, { method: 'POST' })
      .then(function (response) { return response.json(); })
      .then(function (body) {
        token = body['auth-token'];
        seen.token = Boolean(token);
        var state = 'activities/state?activityId=' + enc(activityId) + '&agent=' + enc(JSON.stringify(actor))
          + '&registration=' + enc(registration) + '&stateId=LMS.LaunchData';
        return xapi('GET', state);
      })
      .then(function (response) { return response.json(); })
      .then(function (data) {
        launchData = data;
        seen.launchMode = data.launchMode;
        seen.moveOn = data.moveOn;
        seen.masteryScore = data.masteryScore === undefined ? null : data.masteryScore;
        seen.launchParameters = data.launchParameters === undefined ? null : data.launchParameters;
        seen.returnURL = data.returnURL;
        return xapi('GET', 'agents/profile?agent=' + enc(JSON.stringify(actor)) + '&profileId=cmi5LearnerPreferences');
      })
      .then(function (response) { return response.ok ? response.json() : {}; })
      .then(function (preferences) {
        seen.languagePreference = preferences.languagePreference || null;
        return send('initialized');
      })
      .catch(function (error) {
        seen.error = String(error);
        show();
      });
  }

  function exit() {
    return send('terminated', { duration: duration() }).then(function () {
      if (launchData && launchData.returnURL) location.href = launchData.returnURL;
    });
  }

  function bind(id, action) {
    var button = document.getElementById(id);
    if (button) button.addEventListener('click', action);
  }

  bind('complete', function () { send('completed', { completion: true, duration: duration() }); });
  bind('pass', function () { send('passed', { success: true, score: { scaled: 0.9 }, duration: duration() }); });
  bind('fail', function () { send('failed', { success: false, score: { scaled: 0.4 }, duration: duration() }); });
  // Breaks a session rule: a second `initialized` in one session.
  bind('again', function () { send('initialized'); });
  bind('exit', exit);

  var started = start();
  return { started: started, send: send, exit: exit };
})();
