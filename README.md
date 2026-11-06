# luagee

A sandboxed Lua 5.1 interpreter with LuaJIT-compatible extensions, in safe Rust, for running untrusted scripts
under hard limits. No `unsafe`, no dependencies, and it builds for `wasm32-unknown-unknown`.

```rust
use luagee::{Keys, Limits, Lua};

// The hash key must be secret to the scripts and differ per interpreter: draw it from the host's entropy.
let mut lua = Lua::new(Limits::DEFAULT, Keys { hash: [entropy_0, entropy_1], random: seed })?;
lua.set_print(Box::new(|line| print!("{}", String::from_utf8_lossy(line))));
let results = lua.exec("hello", b"local t = {} for i = 1, 3 do t[i] = i * i end return table.concat(t, ' ')")?;
```

## The dialect

Lua 5.1 with the extensions LuaJIT 2.1 adds to it, as a long-lived LuaJIT 2.1 build accepts them:

- `goto` and labels (LuaJIT's scoping rules), the `bit` library, `0x` hexadecimal floats, `0b` binary literals,
  `\x`, `\z` and `\u{...}` escapes;
- 5.1's function environments: `setfenv`, `getfenv`, `module` with `package.seeall`;
- LuaJIT's library behaviour where it differs from PUC Lua 5.1: `tostring` and `%g` print `nan` for every NaN, `%d`
  and `%x` format 64-bit integers, `string.rep` takes a separator, `tonumber` reads `inf`/`nan`/`0b...`,
  `table.getn`, `foreach`, `foreachi`, `maxn`, `table.new` and `table.clear` (from `require`), `coroutine.isyieldable`,
  and a coroutine may yield through `pcall` and `xpcall`;
- LuaJIT's wording of error messages, including the names of the variables involved (`attempt to call global 'f'
  (a nil value)`), a library function named as its caller referred to it (`local tinsert = table.insert` makes
  `bad argument #1 to 'tinsert'`), and LuaJIT's positions for tail calls (a function's `return f(x)` frame is gone,
  so errors in `f` report from its caller's caller).

The conformance corpus (`crates/luagee/tests/conformance`) holds scripts with the output LuaJIT 2.1 prints for them;
luagee prints the same, byte for byte. `tools/lua_diff/lua_diff.py` re-records them from a LuaJIT build and compares
the two on random programs.

Three other implementations' test suites run as well (`crates/luagee/tests/suites`, each with its provenance and
license): PUC-Rio's Lua 5.1 tests, LuaJIT's test-cleanup tree and the Luau conformance scripts that are Lua 5.1.
`skips.txt` there lists what does not run as written, each with its reason: what the sandbox leaves out (`io`, `os`,
most of `debug`, `string.dump`, `newproxy`, weak tables), the places where LuaJIT, whose behaviour luagee follows,
differs from PUC Lua or Luau, and the differences above.

## The sandbox

A script is treated as hostile input. Whatever it does, the host keeps running:

- Every call runs on fuel counted in VM instructions, and every library function pays for its work: bytes copied,
  pattern-matching steps, values moved, comparisons, allocations, the collector's walks. Fuel is bounded per call and
  per host tick.
- Memory is capped by the interpreter's own count of what it holds (objects, the storage they sit in, stacks,
  compiled code), the same on every platform.
- The collector is incremental and pays fuel for every value it traces and every slot it sweeps, as it goes: its
  work never outruns a call's fuel, and the next call goes on where it stopped.
- Running out of fuel or memory, or loading too much source (per module, in all, or too many modules), ends the call
  with `Error::Limit`. No `pcall` catches it, so no script can retry its way past a bound.
- The other bounds raise Lua's own errors, which scripts may catch: "stack overflow" (Lua calls are frames on the
  interpreter's heap), "C stack overflow" (native code calling back into Lua, a metamethod, a library callback or a
  coroutine resume, nests at most 200 deep, as Lua 5.1's C calls do), "string length overflow", "pattern too
  complex", "chunk has too many syntax levels".
- All Rust recursion is bounded, and so is the native stack it takes. At the default limits the most a script can
  make the interpreter use (`tests/hostile/deepest_native_nesting_*.lua`: native nesting at its bound, then the
  deepest source and patterns at its top) is under 800 KiB on x86-64; under wasmtime, about 340 KiB of its 512 KiB
  stack and 340 KiB of the module's 1 MiB stack. The tests check a 1 MiB native stack, and the gate the
  corpora on three quarters of wasmtime's stack and half the module's. Raising `syntax_depth` or `pattern_depth`
  takes more: up to 2 KiB of native stack a syntax level, 0.3 KiB a pattern level.
- Table keys are hashed with a keyed SipHash the script never sees, so no script can choose keys that collide. The
  hash key and `math.random`'s seed are separate, and both come from the host.
- No IO, OS, FFI, JIT or bytecode: `load`, `loadstring` and `dofile` take source text only. There is no
  `string.dump`, `newproxy`, `io`, `os`, `ffi` or `jit`, and `debug` has only `traceback` and a `getinfo` that reports
  a function's source and lines.
- `require` and `dofile` name a module, never a path: `ModuleName` admits only names that stay inside a folder, and
  the host's `Loader` reads it. The `fs` feature adds `FolderLoader`, which reads `<root>/<name>.lua` and refuses
  symlinks, non-regular files and files past a size bound.
- An error inside a script never panics the host, and the interpreter runs the next call.

The hostile corpus (`crates/luagee/tests/hostile`) holds scripts that attack these bounds; each declares on its first
line how it must end, and the tests check that it does.

## Known differences from LuaJIT

- A coroutine cannot yield inside a metamethod or a library callback (a `table.sort` comparator, a `gsub` function),
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
- A function's limits (200 locals, 60 upvalues) are checked once the whole chunk has parsed: a chunk with a syntax
  error further on reports that error instead.
- Tracebacks and `error`'s levels see Lua functions only: `debug.traceback` lists no `[C]` frames, and a level past a
  native function has no position. A message handler written in Lua has no room to run for a stack overflow, so
  `xpcall` returns "error in error handling" there.
- The length of a table with holes is a border, as Lua promises, but not always the one LuaJIT picks.

## Numbers and determinism

Lua's numbers are IEEE-754 doubles. Floating-point arithmetic happens in the `number` module only: one IEEE
operation per Lua operation, rounded to nearest with ties to even and never fused; the math library goes through one
table there; numbers are printed and read in Rust, never through C's `printf` or `strtod`. Tables iterate in the order
their keys arrived, `tostring` of a table or function prints an id, the collector runs at points the interpreter's
own accounting decides, and `math.random` draws from a stream the host seeds. The same seed and the same calls give
the same output.

## Development

`tools/precheck.sh` is the quick loop (clippy, tests); `tools/gate.sh` is the merge gate (adds the wasm32 build, a
fuzz smoke, `cargo deny`, fmt). `fuzz/fuzz.sh` builds and runs the coverage-guided fuzz targets on stable Rust;
`tools/bench/bench.py` times luagee against reference interpreters.

## License

MIT, see [LICENSE](LICENSE).
