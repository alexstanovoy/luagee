# luagee

A sandboxed Lua 5.1 interpreter with LuaJIT-compatible extensions, in safe Rust, for running untrusted scripts
under hard limits. No `unsafe`, no dependencies, and it runs under WebAssembly (`wasm32-unknown-unknown`,
`wasm32-wasip1`) on one thread and wasm's default stacks: no clock, threads, files or entropy of its own.

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
  deepest source and patterns at its top) is about 800 KiB on x86-64; under wasmtime, about 350 KiB of its 512 KiB
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

`tools/precheck.sh` is the quick loop (clippy, tests); `tools/gate.sh` is the merge gate (adds the wasm32 build and
its audit, a fuzz smoke, the corpora under wasm, `cargo deny`, fmt). `fuzz/fuzz.sh` builds and runs the
coverage-guided fuzz targets on stable Rust; `tools/bench/bench.py` times luagee against reference interpreters.

`tools/wasm.sh corpus` runs the conformance, hostile and fuzz corpora and the borrowed suites under wasmtime for
`wasm32-wasip1`, on one thread and less stack than wasm's defaults (`examples/corpus.rs`, which checks what
`tests/corpus.rs` checks, for any folder of scripts of a kind, and runs `tests/suites.rs` without its threads). `tools/wasm.sh size` reports the size of a minimal embedding
(`examples/embed.rs`), `tools/wasm.sh bench` the speed under wasmtime against native. Its tools (wasmtime,
binaryen's `wasm-opt`) are pinned by version and sha256 and downloaded into `temp/`; the Rust targets come from
rustup.

## Embedding API changes (unreleased, 0.1)

Added public callback argument access (`Args::len/is_empty`, zero-based `Vm::arg`,
`check_*`, `opt_integer`, `opt_string`, `arg_error`), `results`, `new_string` and
`bytes`. Callbacks can create empty tables, read/write raw fields, set table
metatables, create typed userdata, and call Lua without refilling fuel or escaping
the native recursion bound. `charge_fuel` charges application work before doing it.

`Vm::pin/unpin` keep live handles across reentry and between calls. **Breaking:**
`Lua::pin` now returns `Result<(), Error>` and `Lua::unpin` returns
`Result<bool, Error>`: root storage is bounded; inside callbacks, scans and pinning
also consume fuel. Outside calls, `Lua::pin/unpin` work even after script fuel was
exhausted, like other setup operations. Duplicate pins
require matching releases. `begin_tick` also refreshes the fuel available to host
setup operations; callbacks cannot access it. Explicit host collection preserves
the remaining fuel for subsequent setup operations.

Host callbacks are trusted Rust code: bound their work, charge fuel before scans
or copies, and propagate `LuaError::Limit` unchanged. The interpreter checks host
result counts instead of allowing them to read caller stack slots. Callback
arguments are valid only during that callback. Every heap handle must belong to
the interpreter using it and remain live: pin values before reentry if Lua cannot
reach them, including callback captures and fresh allocations. Nested call results
and runtime error objects are automatically rooted until that callback returns;
pin them to retain them longer. Release explicit roots on error paths too.
Allocation alone does not collect;
calling Lua can. Do not cache unpinned handles across calls or interpreters.

`Vm::userdata(data, metatable, retained_bytes)` accounts for the payload's inline
size plus declared owned allocations (such as a vector's capacity), both before
creation and after collection. The host must bound payload allocations before
creating them and declare retained storage accurately. Lua handles inside a payload
are not traced; root them separately. `userdata_ref::<T>` checks the actual Rust
type, independent of the metatable. Seal private metatables with `__metatable`;
scripts cannot change userdata metatables or manufacture typed payloads. The older
`Lua::userdata(Box<dyn Any>, ...)` now counts the boxed payload's inline size too;
use `Vm::userdata` when retaining additional owned allocation.

```rust
use std::rc::Rc;
use luagee::{Keys, Limits, Lua, Value};

let mut lua = Lua::new(Limits::DEFAULT, Keys { hash: [1, 2], random: 3 })?;
let echo = lua.function("echo", Rc::new(|vm, args| {
    let value = vm.check_any(args, 0)?;
    vm.results(&[value])
}))?;
lua.set_global("echo", echo)?;
assert_eq!(lua.exec("example", b"return echo(7)")?, vec![Value::Number(7.0)]);
Ok::<(), luagee::Error>(())
```

### Handle liveness (unreleased fix)

Heap handles carry slot generations. Collecting and reusing an arena slot never
makes an old handle refer to the replacement. Host reads of expired strings or
userdata return `None`; writes, roots, calls and result/error ingress return a Lua
error before a stale handle can enter the interpreter. Handles carry an independent
process-local interpreter tag as well as slot generation. Foreign handles are rejected, including when two interpreters have
identical keys. The tag is not exposed by script hashing, iteration or tostring.
Only one monotonic identity counter is shared; script state remains isolated.

### Callback scope (unreleased fix)

`Vm::call` is only available during a host callback. At top level use `Lua::call`,
which accounts execution against `fuel_per_tick`; calling through `Lua::vm()`
cannot bypass that accounting. `Args` carry an invocation serial: expired contexts
produce nil from `arg` and a Lua error from checked readers instead of inspecting
another callback's frame. Nested call results and runtime error values remain
rooted on the charged callback stack until its return; pin anything retained past
that point. `LuaError` and argument-error constructors must be used or propagated.

### Raw-write fuel (unreleased fix)

Table writes pay for lookup, copying, migration and rebuilding work, rather than
the whole existing table on every write. Compaction pays even when capacity stays
the same. Sequential list construction and hash-to-array migration have linear
fuel cost; replacing an existing element has constant cost. The interpreter uses
one mutation path and charges storage before reservation. Rebuild and migration
budgets are checked before structural changes, so exhaustion leaves valid tables.

### Typed mutation and host stack use (unreleased fix)

`Lua::userdata_mut` and `Vm::userdata_mut` provide a generation-checked typed
mutable borrow, so a host can update payloads without a `RefCell`. The Rust borrow
must end before reentry, and mutations must not grow owned allocations beyond the
declared retained-byte charge or store unrooted Lua handles.

The 200-level native limit includes host callbacks and calls through `Vm::call`;
a callback forwarding into Lua uses two of those levels. Host Rust frames also
consume native/wasm stack. Keep callbacks small and bound their own Rust recursion;
the interpreter cannot bound arbitrary Rust code. The regression runner checks
combined callback reentry and deep parsing on 1 MiB native stacks, 448 KiB wasmtime
engine stacks and 512 KiB module stacks. The engine budget leaves 64 KiB below
wasmtime's default while allowing more layout variation than the former 384 KiB
gate. Embedders with larger callback frames need to verify
their own stack margin.

### Setup and panic boundaries (unreleased fix)

`Lua::vm()` starts a fresh per-call budget for trusted host setup, independent of
fuel left by the preceding script. Setup does not spend tick fuel and cannot run
Lua through `Vm::call`; script execution always uses `Lua::call` and its tick
accounting. Callbacks receive their active `Vm` directly without any refill.
`Lua::string`, like `pin` and `load`, is trusted host setup outside script fuel;
its memory and string-length limits still apply. It neither spends nor refills
execution tick fuel, including after an exhausted tick.

Call cleanup restores callback depth and invocation context even during host Rust
unwinding. A Rust panic crossing an interpreter-call boundary retires it, including
a panic inside a resumed coroutine or explicit collection. Catching a nested panic
inside an outer callback cannot resume script execution. Discard the retired
interpreter and create another one. Subsequent execution,
loading and collection return an error. Normal Lua errors and limit exhaustion
do not retire the interpreter.

Results of repeated `Vm::call` operations within one callback remain rooted on
charged stack storage until that callback returns, even if Rust drops its result
vectors earlier. Long callback loops can therefore hit the memory cap; split
independent work into separate callbacks when those lifetimes are unnecessary.

Opaque interpreter tags make `Value` 24 bytes and table entries 56 bytes. Slot,
frame, capture and container accounting use fixed cross-platform bounds with
compile-time size checks. Interpreter tags use an atomic identity counter and
need no clock, entropy source, thread or filesystem. Exhausting the process-local
identity space fails construction with `interpreter identity space exhausted`;
identities never wrap. Compiled prototypes use a fixed 240-byte header charge,
checked to cover the native Rust header and its `Rc` counters on every target.

## License

MIT, see [LICENSE](LICENSE).

`string.format("%p", value)` uses deterministic, type-tagged local handles, including their generations, instead of
host addresses. Strings with equal bytes share an identity. Nil, booleans and numbers print `NULL`, as in LuaJIT;
width, precision and flags do not affect `%p`.
