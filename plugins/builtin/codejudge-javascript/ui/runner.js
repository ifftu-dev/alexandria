// codejudge JavaScript runner.
//
// Runs a user JS solution against a test's stdin and captures stdout inside a
// QuickJS WebAssembly sandbox (quickjs-emscripten). QuickJS interprets JS in
// wasm — it never calls host eval/Function — so it satisfies the plugin iframe
// CSP (`wasm-unsafe-eval` allowed, `unsafe-eval` and network are not).
//
// I/O contract (matches every codejudge language plugin): the solution reads
// the test input from stdin and writes its answer to stdout. In the sandbox:
//   * `stdin`      — global string: the entire input.
//   * `readLine()` — returns the next line of stdin (without the newline).
//   * `print(...)` / `console.log(...)` — write a space-joined line to stdout.
//
// Exposes (browser): globalThis.CodejudgeRunnerInit() -> Promise<{run}>, using
// the vendored QuickJS bundle on globalThis.CodejudgeQuickJS. Also CommonJS-
// exports makeRunner(QuickJS) so the same core is unit-tested in node.

(function (global) {
  var DEFAULT_TIMEOUT_MS = 2000;

  function makeRunner(QuickJS) {
    function run(source, stdin, opts) {
      opts = opts || {};
      var out = [];
      var vm = QuickJS.newContext();
      try {
        // stdin global + a readLine() helper, defined in-sandbox.
        var sh = vm.newString(stdin || "");
        vm.setProp(vm.global, "stdin", sh);
        sh.dispose();

        var mkLog = function () {
          return vm.newFunction("", function () {
            var parts = [];
            for (var i = 0; i < arguments.length; i++) {
              var v = vm.dump(arguments[i]);
              parts.push(
                typeof v === "string"
                  ? v
                  : v === undefined
                  ? "undefined"
                  : typeof v === "object"
                  ? JSON.stringify(v)
                  : String(v)
              );
            }
            out.push(parts.join(" ") + "\n");
          });
        };

        var printH = mkLog();
        vm.setProp(vm.global, "print", printH);
        var consoleObj = vm.newObject();
        var logH = mkLog();
        vm.setProp(consoleObj, "log", logH);
        vm.setProp(vm.global, "console", consoleObj);
        printH.dispose();
        logH.dispose();
        consoleObj.dispose();

        // readLine() over stdin, implemented in-sandbox off the stdin global.
        var prelude = vm.evalCode(
          "var __pos = 0;\n" +
            "function readLine(){\n" +
            "  if (__pos >= stdin.length) return null;\n" +
            "  var nl = stdin.indexOf('\\n', __pos);\n" +
            "  var line; if (nl === -1) { line = stdin.slice(__pos); __pos = stdin.length; }\n" +
            "  else { line = stdin.slice(__pos, nl); __pos = nl + 1; } return line;\n" +
            "}"
        );
        if (prelude.error) { prelude.error.dispose(); }
        else { prelude.value.dispose(); }

        // Wall-clock timeout via the QuickJS interrupt handler (TLE guard).
        var deadline = now() + (opts.timeoutMs || DEFAULT_TIMEOUT_MS);
        vm.runtime.setInterruptHandler(function () {
          return now() > deadline;
        });

        var res = vm.evalCode(source);
        if (res.error) {
          var errVal = vm.dump(res.error);
          res.error.dispose();
          var msg =
            errVal && errVal.message ? errVal.message : JSON.stringify(errVal);
          if (/interrupted/i.test(String(msg))) {
            return { stdout: out.join(""), error: "time limit exceeded", timedOut: true };
          }
          var name = (errVal && errVal.name) || "Error";
          return { stdout: out.join(""), error: "runtime error: " + name + ": " + msg };
        }
        res.value.dispose();
        return { stdout: out.join(""), error: null };
      } finally {
        vm.dispose();
      }
    }
    return { run: run };
  }

  function now() {
    return typeof Date !== "undefined" ? Date.now() : 0;
  }

  // Browser: build the runner once the vendored QuickJS module is ready.
  function init() {
    var bundle = global.CodejudgeQuickJS;
    if (!bundle || !bundle.getQuickJS) {
      return Promise.reject(new Error("QuickJS runtime not loaded"));
    }
    return bundle.getQuickJS().then(function (QuickJS) {
      global.CodejudgeRunner = makeRunner(QuickJS);
      return global.CodejudgeRunner;
    });
  }

  global.CodejudgeRunnerInit = init;
  if (typeof module !== "undefined" && module.exports) {
    module.exports = { makeRunner: makeRunner };
  }
})(typeof globalThis !== "undefined" ? globalThis : this);
