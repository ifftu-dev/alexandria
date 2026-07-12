// Alexandria plugin bootstrap — host↔plugin protocol v1.
//
// Injected at the top of every plugin HTML response by the `plugin://`
// asset-protocol handler. Defines `window.alex` — the only surface a plugin
// should ever need — and connects it to the host through a MessagePort the
// parent frame sends once the iframe has loaded.
//
// Hardening notes:
// - Deletes `window.__TAURI__` and any Tauri globals before the plugin's
//   own scripts run. The iframe sandbox without `allow-same-origin`
//   already blocks access, this is defense-in-depth.
// - Rejects messages whose api_version doesn't match. New host versions
//   will add optional fields, never break old plugins.
// - Freezes `window.alex` after setup so a hostile plugin script can't
//   replace methods to spoof responses.

(function () {
  'use strict';

  function diag(msg) {
    try {
      window.parent.postMessage({ __alex_diag__: true, msg: String(msg) }, '*');
    } catch (_) {}
  }
  diag('bootstrap.js IIFE start');

  document.addEventListener('securitypolicyviolation', (e) => {
    diag('CSP violation: directive=' + e.violatedDirective + ' blocked=' + e.blockedURI + ' src=' + e.sourceFile);
  });
  window.addEventListener('error', (e) => {
    diag('window error: ' + (e.message || 'unknown') + ' at ' + (e.filename || '?') + ':' + e.lineno);
  });

  // -- Scrub Tauri globals (belt-and-suspenders; sandbox already blocks them).
  try { delete window.__TAURI__; } catch (_) {}
  try { delete window.__TAURI_INTERNALS__; } catch (_) {}
  try { delete window.__TAURI_METADATA__; } catch (_) {}

  const API_VERSION = '1';

  /** @type {MessagePort | null} */
  let hostPort = null;
  const pending = new Map(); // request_id → {resolve, reject}
  let nextRequestId = 1;
  /** @type {Array<() => void>} */
  const portReadyWaiters = [];

  /** @type {((msg: any) => void) | null} */
  let onHostMessageHandler = null;

  function waitForPort() {
    if (hostPort) return Promise.resolve();
    return new Promise((resolve) => portReadyWaiters.push(resolve));
  }

  function postToHost(type, payload) {
    const id = nextRequestId++;
    return waitForPort().then(
      () =>
        new Promise((resolve, reject) => {
          pending.set(id, { resolve, reject });
          hostPort.postMessage({
            api_version: API_VERSION,
            request_id: id,
            type,
            payload: payload || {},
          });
        }),
    );
  }

  function applyTheme(themeVars) {
    if (!themeVars || typeof themeVars !== 'object') return;
    const root = document.documentElement;
    for (const k of Object.keys(themeVars)) {
      const v = themeVars[k];
      if (typeof k === 'string' && k.startsWith('--') && typeof v === 'string') {
        root.style.setProperty(k, v);
      }
    }
  }

  function handleHostMessage(ev) {
    const msg = ev.data;
    if (!msg || typeof msg !== 'object') return;
    if (msg.api_version !== API_VERSION) {
      // Ignore silently; the host will log its side.
      return;
    }
    // Intercept theme tokens at the bootstrap layer so plugin authors can
    // rely on `var(--theme-*)` / `var(--app-*)` being live before their own
    // init handler runs.
    if (msg.type === 'init' && msg.payload && msg.payload.theme) {
      applyTheme(msg.payload.theme);
    } else if (msg.type === 'theme_changed' && msg.payload) {
      applyTheme(msg.payload);
    }
    if (typeof msg.response_id === 'number' && pending.has(msg.response_id)) {
      const entry = pending.get(msg.response_id);
      pending.delete(msg.response_id);
      if (msg.error) {
        entry.reject(new Error(String(msg.error)));
      } else {
        entry.resolve(msg.payload);
      }
      return;
    }
    if (onHostMessageHandler) {
      try {
        onHostMessageHandler(msg);
      } catch (err) {
        console.error('[alex] host message handler threw', err);
      }
    }
  }

  // The parent frame hands over the MessagePort once via a plain window
  // message. We listen on the window — ports cannot traverse sandbox
  // boundaries any other way.
  window.addEventListener('message', (ev) => {
    diag('window message received; has_data=' + !!ev.data + ' has_ports=' + !!(ev.ports && ev.ports[0]));
    if (ev.data && ev.data.__alex_init__ === true && ev.ports && ev.ports[0]) {
      if (hostPort) { diag('hostPort already set — ignoring'); return; }
      hostPort = ev.ports[0];
      hostPort.onmessage = handleHostMessage;
      for (const w of portReadyWaiters.splice(0)) w();
      diag('hostPort set up; waiters=' + portReadyWaiters.length);
    }
  });
  diag('window message listener registered');

  const alex = Object.freeze({
    apiVersion: API_VERSION,

    /** Handshake. Must be called after the plugin has wired its own listeners. */
    ready(declaredCapabilities) {
      return postToHost('ready', {
        declared_capabilities: Array.isArray(declaredCapabilities) ? declaredCapabilities : [],
      });
    },

    /** Request a capability grant from the user. Resolves with `{granted: bool}`. */
    requestCapability(name, reason) {
      return postToHost('request_capability', { name, reason: reason || '' });
    },

    /** Persist per-element state (scoped to this plugin + element). */
    persistState(blob) {
      return postToHost('persist_state', { blob });
    },

    /** Emit a telemetry event (host decides whether to store it). */
    emitEvent(type, payload) {
      return postToHost('emit_event', { type, payload: payload || {} });
    },

    /** Submit a credential-bearing submission (Phase 2+; accepted but ungraded in Phase 1). */
    submit(submission, metadata) {
      return postToHost('submit', { submission, metadata: metadata || {} });
    },

    /**
     * Open the host's native file picker (the sandboxed iframe cannot show one
     * itself). User-initiated file selection is its own consent, so no
     * capability grant is required. Resolves with
     * `{ files: [{ name, type, size, data: Uint8Array }] }` (empty if cancelled).
     */
    pickFiles(options) {
      return postToHost('pick_files', options || {});
    },

    /** Mark the element as complete for interactive plugins. */
    complete(progressFraction, optionalAdvisoryScore) {
      return postToHost('complete', {
        progress_fraction: typeof progressFraction === 'number' ? progressFraction : 1,
        optional_advisory_score:
          typeof optionalAdvisoryScore === 'number' ? optionalAdvisoryScore : null,
      });
    },

    /** Register a handler for unsolicited host messages (e.g. capability_revoked). */
    onHost(handler) {
      onHostMessageHandler = typeof handler === 'function' ? handler : null;
    },
  });

  Object.defineProperty(window, 'alex', {
    value: alex,
    writable: false,
    configurable: false,
    enumerable: true,
  });
})();
