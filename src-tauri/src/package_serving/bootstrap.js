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
// Content can make windows this script was never loaded into: a new frame
// (about:blank), a frame with a `srcdoc` document, a document made from a
// javascript: address. Each has the browser's interfaces back. So:
//
// - a new frame is scrubbed the moment it exists, from its synchronous
//   `load` event, before the script that made it can reach it;
// - a `srcdoc` document is given this script as its first element;
// - a javascript: address still runs, but its result never replaces the
//   document. Pages are served with `require-trusted-types-for 'script'` and
//   this script's default policy turns such an address into one that runs
//   the same code and discards the result. Every other script sink passes
//   through unchanged, so legacy content keeps working.
//
// WebKit sends `<link rel=preload as=fetch>` to any origin whatever the
// content policy says. The handler strips such an `as` from served markup;
// this script does the same for markup and attributes content adds later.
//
// This is one layer. The content policy, the frame policies and the launch
// prefix are each enforced on their own; none relies on this script.

(function () {
  'use strict';

  var MESSAGE_SOURCE = 'alexandria-package';
  var current = document.currentScript;
  var BOOTSTRAP_TAG = current && current.src ? '<script src="' + current.src + '"></script>' : '';

  // `as=fetch` in a `<link>` tag, renamed so the tag preloads nothing (the
  // handler does the same to served markup).
  var FETCH_PRELOAD = /(<link\b[^>]*?[\s\/])as(\s*=\s*(?:"\s*fetch\s*"|'\s*fetch\s*'|fetch(?=[\s\/>])))/gi;
  function withoutFetchPreloads(markup) {
    return String(markup).replace(FETCH_PRELOAD, '$1data-alexandria-as$2');
  }
  function isFetch(value) {
    return String(value).trim().toLowerCase() === 'fetch';
  }

  function withBootstrap(markup) {
    var text = String(markup);
    if (!BOOTSTRAP_TAG || text.indexOf(BOOTSTRAP_TAG) === 0) return text;
    return BOOTSTRAP_TAG + text;
  }

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

  // The default Trusted Types policy (see the header). Without it a page
  // served with the Trusted Types requirement could not use any string sink.
  var POLICY = {
    createHTML: function (input, _type, sink) {
      var markup = withoutFetchPreloads(input);
      return sink === 'HTMLIFrameElement srcdoc' ? withBootstrap(markup) : markup;
    },
    createScript: function (input, _type, sink) {
      if (sink !== 'Location href') return input;
      return 'void eval(' + JSON.stringify(String(input)) + ')';
    },
    createScriptURL: function (input) {
      return input;
    },
  };

  function trustedTypes(win) {
    try {
      var factory = win.trustedTypes;
      if (factory && !factory.defaultPolicy) factory.createPolicy('default', POLICY);
    } catch (_) {}
  }

  function isFrame(node) {
    return Boolean(node) && (node.tagName === 'IFRAME' || node.tagName === 'FRAME');
  }

  // Whether a frame is on its way to another document. Its first blank
  // window is then kept for that document, which brings its own bootstrap;
  // a Trusted Types policy made now would be left behind in WebKit and the
  // new page could not make its own.
  function leaving(element) {
    try {
      if (element.getAttribute('srcdoc') !== null) return true;
      var src = element.getAttribute('src');
      return Boolean(src) && !/^\s*about:blank\s*$/i.test(src);
    } catch (_) {
      return false;
    }
  }

  // A frame content added or changed: its window is scrubbed, a `srcdoc`
  // document gets the bootstrap, a javascript: address becomes a blank page.
  function guardFrame(element) {
    try {
      var srcdoc = element.getAttribute('srcdoc');
      if (srcdoc !== null && srcdoc.indexOf(BOOTSTRAP_TAG) !== 0) element.setAttribute('srcdoc', withBootstrap(srcdoc));
      var src = element.getAttribute('src');
      if (src && /^\s*javascript:/i.test(src)) element.setAttribute('src', 'about:blank');
    } catch (_) {}
    scrubFrame(element, element.contentWindow);
  }

  function scrubFrame(element, win) {
    try {
      scrub(win, !leaving(element));
    } catch (_) {}
  }

  // Watches `root` (a document or shadow root) for frames: synchronously
  // through their `load` event, which a new frame fires as it is inserted,
  // and through a mutation observer for markup the parser or script adds,
  // which runs before any `srcdoc` document can load.
  function watch(win, root) {
    try {
      root.addEventListener('load', function (event) {
        if (isFrame(event.target)) guardFrame(event.target);
      }, true);
    } catch (_) {}
    try {
      new win.MutationObserver(function (records) {
        records.forEach(function (record) {
          if (record.type === 'attributes') {
            if (isFrame(record.target)) guardFrame(record.target);
            return;
          }
          record.addedNodes.forEach(function (node) {
            if (node.nodeType !== 1) return;
            if (isFrame(node)) guardFrame(node);
            if (node.querySelectorAll) Array.prototype.forEach.call(node.querySelectorAll('iframe, frame'), guardFrame);
          });
        });
      }).observe(root, { childList: true, subtree: true, attributes: true, attributeFilter: ['srcdoc', 'src'] });
    } catch (_) {}
  }

  function wrapSetter(proto, name, change) {
    try {
      var descriptor = Object.getOwnPropertyDescriptor(proto, name);
      if (!descriptor || !descriptor.set) return;
      var set = descriptor.set;
      Object.defineProperty(proto, name, {
        get: descriptor.get,
        set: function (value) {
          set.call(this, change(this, value));
        },
        enumerable: descriptor.enumerable,
        configurable: false,
      });
    } catch (_) {}
  }

  function wrapMethod(proto, name, wrap) {
    try {
      var original = proto && proto[name];
      if (typeof original !== 'function') return;
      Object.defineProperty(proto, name, { value: wrap(original), writable: false, configurable: false });
    } catch (_) {}
  }

  function isLink(element) {
    return Boolean(element) && String(element.localName).toLowerCase() === 'link';
  }

  // Every way script sets a `<link>`'s `as` attribute, with `fetch` turned
  // into an empty value (a preload with no `as` sends nothing).
  function noFetchPreloads(win) {
    var clean = function (element, name, value) {
      return isLink(element) && String(name).toLowerCase() === 'as' && isFetch(value) ? '' : value;
    };
    if (win.HTMLLinkElement) {
      wrapSetter(win.HTMLLinkElement.prototype, 'as', function (element, value) {
        return isFetch(value) ? '' : value;
      });
    }
    var elementProto = win.Element && win.Element.prototype;
    wrapMethod(elementProto, 'setAttribute', function (original) {
      return function (name, value) {
        return original.call(this, name, clean(this, name, value));
      };
    });
    wrapMethod(elementProto, 'setAttributeNS', function (original) {
      return function (namespace, name, value) {
        return original.call(this, namespace, name, clean(this, String(name).split(':').pop(), value));
      };
    });
    var attrProto = win.Attr && win.Attr.prototype;
    var attrValue = attrProto && Object.getOwnPropertyDescriptor(attrProto, 'value');
    var cleanAttr = function (attr, element) {
      try {
        if (isLink(element) && String(attr.localName).toLowerCase() === 'as' && isFetch(attr.value)) attrValue.set.call(attr, '');
      } catch (_) {}
    };
    ['value', 'nodeValue', 'textContent'].forEach(function (name) {
      var proto = name === 'value' ? attrProto : win.Node && win.Node.prototype;
      wrapSetter(proto, name, function (node, value) {
        if (node && node.nodeType === 2 && isLink(node.ownerElement) && String(node.localName).toLowerCase() === 'as' && isFetch(value)) return '';
        return value;
      });
    });
    var attach = function (original) {
      return function (attr) {
        cleanAttr(attr, this);
        return original.apply(this, arguments);
      };
    };
    wrapMethod(elementProto, 'setAttributeNode', attach);
    wrapMethod(elementProto, 'setAttributeNodeNS', attach);
    var mapProto = win.NamedNodeMap && win.NamedNodeMap.prototype;
    ['setNamedItem', 'setNamedItemNS'].forEach(function (name) {
      wrapMethod(mapProto, name, function (original) {
        return function (attr) {
          var result = original.apply(this, arguments);
          cleanAttr(attr, attr && attr.ownerElement);
          return result;
        };
      });
    });
  }

  // Once per document, not per window: a frame scrubbed while it still
  // showed its first blank document keeps the same window when it moves on
  // to a page at the same origin, and that page needs its own pass.
  function scrub(win, policy) {
    if (!win) return;
    try {
      var doc = win.document;
      if (doc.__alexandriaPackageScrubbed) return;
      Object.defineProperty(doc, '__alexandriaPackageScrubbed', { value: true });
    } catch (_) {
      return;
    }

    if (policy !== false) trustedTypes(win);

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

    // A preload `as` of `fetch` is never set (see the header).
    noFetchPreloads(win);

    // Frames in this document and in every shadow tree content attaches.
    watch(win, win.document);
    try {
      var elementProto = win.Element && win.Element.prototype;
      var attachShadow = elementProto && elementProto.attachShadow;
      if (attachShadow) {
        Object.defineProperty(elementProto, 'attachShadow', {
          value: function () {
            var root = attachShadow.apply(this, arguments);
            watch(win, root);
            return root;
          },
          writable: false,
          configurable: false,
        });
      }
    } catch (_) {}

    // A frame the content makes without loading a document from the package
    // (about:blank, a written document) gets no bootstrap of its own; scrub
    // it when the content reaches for its window, too.
    try {
      var frameProto = win.HTMLIFrameElement && win.HTMLIFrameElement.prototype;
      ['contentWindow', 'contentDocument'].forEach(function (name) {
        var descriptor = Object.getOwnPropertyDescriptor(frameProto, name);
        if (!descriptor || !descriptor.get) return;
        var original = descriptor.get;
        Object.defineProperty(frameProto, name, {
          get: function () {
            var value = original.call(this);
            scrubFrame(this, name === 'contentWindow' ? value : value && value.defaultView);
            return value;
          },
          configurable: false,
        });
      });
    } catch (_) {}
  }

  scrub(window);
})();
