// Alexandria package bootstrap.
//
// The package scheme handler links this script as the first element of every
// HTML document it serves, so it runs before any package script. Package
// script is untrusted. This script removes what the content policy header
// does not govern:
//
// - the app's command bridge (Tauri globals and the native message channels
//   wry installs: `webkit.messageHandlers`, `chrome.webview`, `ipc`);
// - network-capable interfaces outside `connect-src`: peer connections
//   first, then cross-document channels that would let two launches on one
//   origin talk;
// - device capabilities: camera, microphone, location, clipboard read,
//   full screen, sharing and hardware access;
// - service worker registration.
//
// It also turns `window.open` and links aimed at another window into a
// navigation of the frame that asked, and a full-screen request into a
// focus-mode request to the host. It tells the host, with a `blocked` message
// naming the kind of thing, when content asks for a pop-up, a dialog, a
// plug-in or a resource from off the package origin, so the player can say
// why the content may look broken.
//
// This is one layer. The content policy, the frame policies and the launch
// prefix are each enforced on their own; none relies on this script.

(function () {
  'use strict';

  var MESSAGE_SOURCE = 'alexandria-package';

  function hide(target, name) {
    if (!target) return;
    try {
      delete target[name];
    } catch (_) {}
    try {
      if (name in target) {
        Object.defineProperty(target, name, {
          value: undefined,
          writable: false,
          configurable: false,
        });
      }
    } catch (_) {}
    try {
      if (target[name] !== undefined) target[name] = undefined;
    } catch (_) {}
  }

  function refuse(name) {
    return function () {
      var error = new DOMException(name + ' is not available to package content', 'NotAllowedError');
      return Promise.reject(error);
    };
  }

  function tellHost(win, type, what, detail) {
    var message = { source: MESSAGE_SOURCE, type: type };
    if (what) message.what = what;
    if (detail) message.detail = String(detail).slice(0, 200);
    try {
      win.top.postMessage(message, '*');
    } catch (_) {}
  }

  // The player tells the learner when content needed something that is not
  // available here. Only the kind of thing and a host name travel.
  function blocked(win, what, detail) {
    tellHost(win, 'blocked', what, detail);
  }

  function hostOf(address) {
    try {
      return new URL(address).host;
    } catch (_) {
      return '';
    }
  }

  function scrub(win) {
    if (!win) return;
    try {
      if (win.__alexandriaPackageScrubbed) return;
      Object.defineProperty(win, '__alexandriaPackageScrubbed', { value: true });
    } catch (_) {
      return;
    }

    // Command bridge.
    try {
      Object.getOwnPropertyNames(win).forEach(function (name) {
        if (name.indexOf('__TAURI') === 0) hide(win, name);
      });
    } catch (_) {}
    try {
      if (win.webkit) hide(win.webkit, 'messageHandlers');
    } catch (_) {}
    hide(win, 'webkit');
    try {
      if (win.chrome) hide(win.chrome, 'webview');
    } catch (_) {}
    hide(win, 'ipc');

    // Peer connections and other channels the content policy does not govern.
    [
      'RTCPeerConnection',
      'webkitRTCPeerConnection',
      'mozRTCPeerConnection',
      'RTCDataChannel',
      'RTCIceCandidate',
      'RTCSessionDescription',
      'RTCRtpSender',
      'RTCRtpReceiver',
      'RTCRtpTransceiver',
      'RTCDtlsTransport',
      'RTCIceTransport',
      'RTCSctpTransport',
      'RTCCertificate',
      'WebTransport',
      'BroadcastChannel',
      'SharedWorker',
      'PresentationRequest',
      'Notification',
      'PaymentRequest',
    ].forEach(function (name) {
      hide(win, name);
    });

    // Device capabilities.
    var nav = win.navigator;
    var navProto = win.Navigator && win.Navigator.prototype;
    [
      'mediaDevices',
      'getUserMedia',
      'webkitGetUserMedia',
      'geolocation',
      'share',
      'canShare',
      'credentials',
      'bluetooth',
      'usb',
      'serial',
      'hid',
      'xr',
      'wakeLock',
      'presentation',
      'requestMIDIAccess',
      'requestMediaKeySystemAccess',
    ].forEach(function (name) {
      hide(navProto, name);
      hide(nav, name);
    });
    try {
      var clipboardProto = win.Clipboard && win.Clipboard.prototype;
      hide(clipboardProto, 'read');
      hide(clipboardProto, 'readText');
    } catch (_) {}

    // Service workers: registration always fails.
    try {
      var container = win.ServiceWorkerContainer && win.ServiceWorkerContainer.prototype;
      if (container) {
        Object.defineProperty(container, 'register', {
          value: refuse('Service worker registration'),
          writable: false,
          configurable: false,
        });
      }
    } catch (_) {}

    // Full screen becomes a focus-mode request; the frame never gets it.
    var requestFocus = function () {
      tellHost(win, 'focus-mode-request');
      return Promise.reject(
        new DOMException('Full screen is not available to package content', 'NotAllowedError')
      );
    };
    [
      [win.Element, ['requestFullscreen', 'webkitRequestFullscreen', 'webkitRequestFullScreen', 'mozRequestFullScreen', 'msRequestFullscreen']],
      [win.HTMLVideoElement, ['webkitEnterFullscreen', 'webkitEnterFullScreen']],
    ].forEach(function (entry) {
      var proto = entry[0] && entry[0].prototype;
      if (!proto) return;
      entry[1].forEach(function (name) {
        try {
          Object.defineProperty(proto, name, {
            value: requestFocus,
            writable: false,
            configurable: false,
          });
        } catch (_) {}
      });
    });

    // A new window opens in the frame that asked for it. The timer is taken
    // now, before content can replace it.
    var later = win.setTimeout.bind(win);
    try {
      Object.defineProperty(win, 'open', {
        value: function (url) {
          if (url === undefined || url === null || String(url) === '') return null;
          var target;
          try {
            target = new win.URL(String(url), win.document.baseURI).href;
          } catch (_) {
            return null;
          }
          // The host is told first and the frame navigates a task later: a
          // message from a document already on its way out can arrive with
          // no source, and the host drops messages it cannot place.
          blocked(win, 'popup');
          later(function () {
            try {
              win.location.assign(target);
            } catch (_) {}
          }, 0);
          return win;
        },
        writable: false,
        configurable: false,
      });
    } catch (_) {}

    // The player frame has no modal dialogs; say so instead of failing
    // silently.
    [
      ['alert', undefined],
      ['confirm', false],
      ['prompt', null],
    ].forEach(function (entry) {
      try {
        Object.defineProperty(win, entry[0], {
          value: function () {
            blocked(win, 'dialog');
            return entry[1];
          },
          writable: false,
          configurable: false,
        });
      } catch (_) {}
    });

    // Anything the content policy turns away: a plug-in, or a resource from
    // off the package origin.
    try {
      win.document.addEventListener('securitypolicyviolation', function (event) {
        var directive = event.effectiveDirective || event.violatedDirective || '';
        if (directive.indexOf('object-src') === 0) blocked(win, 'plugin');
        else if (directive.indexOf('script-src') === 0 && !/^https?:/.test(event.blockedURI || '')) return;
        else blocked(win, 'remote-resource', hostOf(event.blockedURI));
      });
    } catch (_) {}

    // Links and forms aimed at another window stay in this frame.
    try {
      var retarget = function (event) {
        var node = event.target;
        while (node && node.nodeType === 1) {
          var tag = node.tagName;
          if (tag === 'A' || tag === 'AREA' || tag === 'FORM') {
            var target = (node.getAttribute('target') || '').toLowerCase();
            if (target && target !== '_self' && target !== '_parent' && target !== '_top') {
              node.setAttribute('target', '_self');
            }
            return;
          }
          node = node.parentNode;
        }
      };
      win.document.addEventListener('click', retarget, true);
      win.document.addEventListener('submit', retarget, true);
    } catch (_) {}

    // A frame the content makes without loading a document from the package
    // (about:blank, a written document) gets no bootstrap of its own; scrub
    // it when the content reaches for its window.
    try {
      var frameProto = win.HTMLIFrameElement && win.HTMLIFrameElement.prototype;
      ['contentWindow', 'contentDocument'].forEach(function (name) {
        var descriptor = Object.getOwnPropertyDescriptor(frameProto, name);
        if (!descriptor || !descriptor.get) return;
        var original = descriptor.get;
        Object.defineProperty(frameProto, name, {
          get: function () {
            var value = original.call(this);
            try {
              scrub(name === 'contentWindow' ? value : value && value.defaultView);
            } catch (_) {}
            return value;
          },
          configurable: false,
        });
      });
    } catch (_) {}
  }

  scrub(window);
})();
