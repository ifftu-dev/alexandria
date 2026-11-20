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
// focus-mode request to the host.
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

  function tellHost(win, type) {
    try {
      win.top.postMessage({ source: MESSAGE_SOURCE, type: type }, '*');
    } catch (_) {}
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

    // A new window opens in the frame that asked for it.
    try {
      Object.defineProperty(win, 'open', {
        value: function (url) {
          if (url === undefined || url === null || String(url) === '') return null;
          try {
            win.location.assign(new win.URL(String(url), win.document.baseURI).href);
          } catch (_) {
            return null;
          }
          return win;
        },
        writable: false,
        configurable: false,
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
