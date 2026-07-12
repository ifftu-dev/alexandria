// Minimal browser stub for node's `util` (printf only touches format/inspect).
exports.format = function (f) {
  var args = Array.prototype.slice.call(arguments, 1), i = 0;
  return String(f).replace(/%[sdj%]/g, function (m) {
    if (m === '%%') return '%';
    return i < args.length ? String(args[i++]) : m;
  });
};
exports.inspect = function (x) { try { return JSON.stringify(x); } catch (_) { return String(x); } };
