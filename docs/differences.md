# Differences from the reference implementations

What language and library behaviour luagee follows (Lua 5.1 with LuaJIT 2.1's extensions), how that is checked, and where it
deliberately differs. For the safety and determinism guarantees see [guarantees.md](guarantees.md).

## The dialect

Lua 5.1 with the extensions LuaJIT 2.1 adds to it, as a long-lived LuaJIT 2.1 build accepts them:

- `goto` and labels (LuaJIT's scoping rules), the `bit` library, `0x` hexadecimal floats, `0b` binary literals,
  `\x`, `\z` and `\u{...}` escapes;
- 5.1's function environments: `setfenv`, `getfenv`, `module` with `package.seeall`;
- LuaJIT's library behaviour where it differs from PUC Lua 5.1: `tostring` and `%g` print `nan` for every NaN, `%d`
  and `%x` format 64-bit integers, `string.rep` takes a separator, `tonumber` reads `inf`/`nan`/`0b...`,
  `table.getn`, `foreach`, `foreachi`, `maxn`, `table.move`, `table.new` and `table.clear` (from `require`), `coroutine.isyieldable`,
  and a coroutine may yield through `pcall` and `xpcall`;
- LuaJIT's wording of error messages, including the names of the variables involved (`attempt to call global 'f'
  (a nil value)`), a library function named as its caller referred to it (`local tinsert = table.insert` makes
  `bad argument #1 to 'tinsert'`), and LuaJIT's positions for tail calls (a function's `return f(x)` frame is gone,
  so errors in `f` report from its caller's caller).

Repeated `string.format` flags are accepted as in LuaJIT: flag count is bounded by the template's string limit and
charged in fuel before parsing, rather than rejected by a separate flag-count limit. Width and precision each consume
at most two decimal digits, keeping generated fields bounded to 99; an extra digit is an invalid conversion, as in
LuaJIT. This accepts repeated zero flags without treating them as a huge width.

The conformance corpus (`crates/luagee/tests/conformance`) holds scripts with the output LuaJIT 2.1 prints for them;
luagee prints the same, byte for byte. `tools/lua_diff/lua_diff.py` re-records them from a LuaJIT build and compares
the two on random programs.

Three other implementations' test suites run as well (`crates/luagee/tests/suites`, each with its provenance and
license): PUC-Rio's Lua 5.1 tests, LuaJIT's test-cleanup tree and the Luau conformance scripts that are Lua 5.1.
`skips.txt` there lists what does not run as written, each with its reason: what the sandbox leaves out (`io`, `os`,
most of `debug`, `string.dump`, `newproxy`, weak tables), the places where LuaJIT, whose behaviour luagee follows,
differs from PUC Lua or Luau, and the differences above.

## Known differences from LuaJIT

- A coroutine cannot yield inside a metamethod or a library callback (a `table.sort` comparator, a `gsub` function,
  or an `__eq` handler called by `table.move`),
  as in Lua 5.1. Yielding through `pcall` and `xpcall` works.
- `__mode` and `__gc` have no effect: weak tables hold their entries, and nothing is finalized.
- Native code calling back into Lua (metamethods, library callbacks, nested coroutine resumes) stops at 200
  levels ("C stack overflow"), as in Lua 5.1; LuaJIT runs metamethods as Lua frames and goes deeper.
- Syntax that only recent LuaJIT builds accept (`//`, `&`, `<<`, `!=`, `+=`) does not parse.
- Bounds raise errors where LuaJIT would allocate without limit (a 10 GB `string.rep`, for one).
- Scripts cannot steer the collector: `collectgarbage("stop")`, `"step"` and the tuning options change nothing, and
  `collectgarbage("count")` and `gcinfo` count the storage the interpreter keeps for reuse, so they do not fall back
  as LuaJIT's do once garbage is freed.
- `load` reads its reader to the end before compiling, so a syntax error does not stop the reading early.
- A function's limits (200 locals, and by default 60 upvalues) are checked once the whole chunk has parsed: a chunk
  with a syntax error further on reports that error instead. The upvalue limit is the host's (`Limits::function_upvalues`,
  at most 255, which stock Lua's own maximum is too); nested functions count towards it as in stock Lua, a function
  holding the upvalues its inner functions use.
- Tracebacks and `error`'s levels see Lua functions only: `debug.traceback` lists no `[C]` frames, and a level past a
  native function has no position. A message handler written in Lua has no room to run for a stack overflow, so
  `xpcall` returns "error in error handling" there.
- The length of a table with holes is a border, as Lua promises, but not always the one LuaJIT picks.
