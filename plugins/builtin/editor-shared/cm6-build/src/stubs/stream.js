// Minimal browser stub for node's `stream` — JSCPP/printf only does
// `arg instanceof Stream`, which is always false for our string I/O.
function Stream() {}
exports.Stream = Stream;
exports.Readable = Stream;
exports.Writable = Stream;
