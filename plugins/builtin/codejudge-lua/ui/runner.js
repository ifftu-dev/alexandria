// codejudge Lua runner.
//
// Runs a user Lua solution against a test's stdin and captures stdout inside
// the wasmoon Lua VM (Lua 5.4 compiled to WebAssembly). Lua runs *inside* the
// wasm sandbox — no JS eval/Function — so it satisfies the plugin iframe CSP
// (`wasm-unsafe-eval` allowed, `unsafe-eval` and network are not). The wasm is
// embedded in vendor/lua.js (built offline) and instantiated with no fetch.
//
// (We previously used fengari, a pure-JS Lua VM, but it needs `unsafe-eval`
// internally, which the plugin CSP forbids — hence the move to wasmoon.)
//
// I/O contract (matches every codejudge language plugin): the solution reads
// the test input from stdin (io.read / io.lines) and writes to stdout
// (print / io.write).
//
// Exposes globalThis.CodejudgeRunnerInit() -> Promise<{ run }>, where run is
// async: run(source, stdin, opts) -> { stdout, error, timedOut? }.

(function (global) {
  var DEFAULT_TIMEOUT_MS = 2000;

  // Lua prelude: rebinds io.read / io.lines / print / io.write to a host-backed
  // stdin string + an output sink, and installs an instruction-count hook that
  // calls a host guard which throws past a wall-clock deadline (TLE). The hook
  // must be set in the SAME chunk as the user code to apply to it, so the
  // runner concatenates PRELUDE + "\n" + source into a single doString.
  var PRELUDE = [
    'local __in = __get_stdin()',
    'local __pos = 1',
    'local function __next_line()',
    '  if __pos > #__in then return nil end',
    '  local nl = string.find(__in, "\\n", __pos, true)',
    '  local line',
    '  if nl then line = string.sub(__in, __pos, nl - 1); __pos = nl + 1',
    '  else line = string.sub(__in, __pos); __pos = #__in + 1 end',
    '  return line',
    'end',
    'io = io or {}',
    'function io.write(...) local n=select("#",...) for i=1,n do __emit(tostring((select(i,...)))) end end',
    'function io.read(fmt)',
    '  fmt = fmt or "l"',
    '  if fmt=="*a" or fmt=="a" then local r=string.sub(__in,__pos); __pos=#__in+1; return r',
    '  elseif fmt=="*n" or fmt=="n" then local l=__next_line(); return l and tonumber(l) or nil',
    '  else return __next_line() end',
    'end',
    'function io.lines() return function() return __next_line() end end',
    'read = io.read',
    'function print(...) local n=select("#",...) local p={} for i=1,n do p[i]=tostring((select(i,...))) end __emit(table.concat(p,"\\t").."\\n") end',
    'debug.sethook(function() __guard() end, "", 100000)',
  ].join("\n");

  function makeRunner(factory) {
    async function run(source, stdin, opts) {
      opts = opts || {};
      var timeoutMs = opts.timeoutMs || DEFAULT_TIMEOUT_MS;
      var out = [];
      var deadline = now() + timeoutMs;
      var engine = await factory.createEngine();
      try {
        engine.global.set("__guard", function () {
          if (now() > deadline) throw new Error("time limit exceeded");
        });
        engine.global.set("__emit", function (s) { out.push(String(s)); });
        engine.global.set("__get_stdin", function () { return stdin || ""; });
        await engine.doString(PRELUDE + "\n" + source);
        return { stdout: out.join(""), error: null };
      } catch (e) {
        var msg = String(e && e.message ? e.message : e);
        if (/time limit exceeded/.test(msg)) {
          return { stdout: out.join(""), error: "time limit exceeded", timedOut: true };
        }
        return { stdout: out.join(""), error: "runtime error: " + msg.split("\n")[0].slice(0, 160) };
      } finally {
        try { engine.global.close(); } catch (e) {}
      }
    }
    return { run: run };
  }

  function now() {
    return typeof Date !== "undefined" ? Date.now() : 0;
  }

  // Build the runner once the vendored wasmoon module is loaded. The factory
  // loads the wasm a single time; each run gets a fresh Lua state for isolation.
  function init() {
    var lua = global.CodejudgeLua;
    if (!lua || !lua.newFactory) {
      return Promise.reject(new Error("Lua runtime not loaded"));
    }
    var factory = lua.newFactory();
    global.CodejudgeRunner = makeRunner(factory);
    return Promise.resolve(global.CodejudgeRunner);
  }

  global.CodejudgeRunnerInit = init;
})(typeof globalThis !== "undefined" ? globalThis : this);
