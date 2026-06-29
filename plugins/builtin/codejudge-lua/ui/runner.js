// codejudge Lua runner.
//
// Runs a user Lua solution against a test's stdin and captures stdout, entirely
// inside the fengari Lua VM (pure JS — no eval, no wasm, no network), so it is
// safe under the plugin iframe CSP. The I/O contract matches every codejudge
// language plugin: the solution reads from stdin (io.read / io.lines) and writes
// to stdout (print / io.write).
//
// Exposes globalThis.CodejudgeRunner = { run(source, stdin, opts) }.
// Also CommonJS-exports runWith(fengari, ...) so the same core is unit-tested
// in node against the `fengari` package.

(function (global) {
  // Lua prelude: rebinds print / io.write / io.read to host-backed stdin+stdout
  // so they work in-browser (fengari's default io talks to a tty that isn't
  // there). __emit(str) and __get_stdin() are JS functions pushed by the host.
  const PRELUDE = `
    local __in = __get_stdin()
    local __pos = 1
    local function __next_line()
      if __pos > #__in then return nil end
      local nl = string.find(__in, "\\n", __pos, true)
      local line
      if nl then line = string.sub(__in, __pos, nl - 1); __pos = nl + 1
      else line = string.sub(__in, __pos); __pos = #__in + 1 end
      return line
    end
    io = io or {}
    function io.write(...)
      local n = select("#", ...)
      for i = 1, n do __emit(tostring((select(i, ...)))) end
    end
    function io.read(fmt)
      fmt = fmt or "l"
      if fmt == "*a" or fmt == "a" then
        local r = string.sub(__in, __pos); __pos = #__in + 1; return r
      elseif fmt == "*n" or fmt == "n" then
        local l = __next_line(); return l and tonumber(l) or nil
      else
        return __next_line()
      end
    end
    function io.lines()
      return function() return __next_line() end
    end
    read = io.read
    function print(...)
      local n = select("#", ...)
      local parts = {}
      for i = 1, n do parts[i] = tostring((select(i, ...))) end
      __emit(table.concat(parts, "\\t") .. "\\n")
    end
  `;

  // Default: abort runaway programs after this many VM instructions (TLE guard;
  // fengari is synchronous so an infinite loop would otherwise hang the iframe).
  const DEFAULT_MAX_INSTR = 40_000_000;

  function runWith(fengari, source, stdin, opts) {
    opts = opts || {};
    const maxInstr = opts.maxInstr || DEFAULT_MAX_INSTR;
    const { lua, lauxlib, lualib, to_luastring } = fengari;

    const out = [];
    const L = lauxlib.luaL_newstate();
    lualib.luaL_openlibs(L);

    // Host stdout sink.
    lua.lua_pushcfunction(L, function (L) {
      out.push(lua.lua_tojsstring(L, 1));
      return 0;
    });
    lua.lua_setglobal(L, to_luastring("__emit"));

    // Host stdin source.
    lua.lua_pushcfunction(L, function (L) {
      lua.lua_pushstring(L, to_luastring(stdin || ""));
      return 1;
    });
    lua.lua_setglobal(L, to_luastring("__get_stdin"));

    // Instruction-count hook → cooperative timeout.
    lua.lua_sethook(
      L,
      function () {
        lauxlib.luaL_error(L, to_luastring("time limit exceeded"));
      },
      lua.LUA_MASKCOUNT,
      maxInstr
    );

    const fail = (stage) => {
      const msg = lua.lua_tojsstring(L, -1) || "unknown error";
      return { stdout: out.join(""), error: `${stage}: ${msg}` };
    };

    if (lauxlib.luaL_dostring(L, to_luastring(PRELUDE)) !== lua.LUA_OK) {
      return fail("prelude error");
    }
    if (lauxlib.luaL_dostring(L, to_luastring(source)) !== lua.LUA_OK) {
      // Distinguish the TLE sentinel from ordinary runtime errors.
      const msg = lua.lua_tojsstring(L, -1) || "";
      if (msg.indexOf("time limit exceeded") !== -1) {
        return { stdout: out.join(""), error: "time limit exceeded", timedOut: true };
      }
      return fail("runtime error");
    }
    return { stdout: out.join(""), error: null };
  }

  function run(source, stdin, opts) {
    if (!global.fengari) throw new Error("fengari runtime not loaded");
    return runWith(global.fengari, source, stdin, opts);
  }

  global.CodejudgeRunner = { run, runWith };
  if (typeof module !== "undefined" && module.exports) {
    module.exports = { run, runWith, PRELUDE };
  }
})(typeof globalThis !== "undefined" ? globalThis : this);
