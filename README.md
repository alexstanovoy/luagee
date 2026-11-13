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

## Numbers and determinism

Lua's numbers are IEEE-754 doubles. Floating-point arithmetic happens in the `number` module only: one IEEE
operation per Lua operation, rounded to nearest with ties to even and never fused; the math library goes through one
table there; numbers are printed and read in Rust, never through C's `printf` or `strtod`. Tables iterate in the order
their keys arrived, `tostring` of a table or function prints an id, the collector runs at points the interpreter's
own accounting decides, and `math.random` draws from a stream the host seeds. The same seed and the same calls give
the same output.

Numeric-for loops preserve LuaJIT's signed-zero direction for real steps. A NaN step always uses the Lua 5.1
nonpositive comparison (`limit <= index`), independent of its sign or payload. This intentionally differs from
LuaJIT where its sign-bit test makes unequal-bound NaN loops depend on how that NaN was produced or the platform.
Equal-bound and NaN-bound cases remain reference-tested; unequal-bound NaN expectations test this explicit rule.

`string.format("%p", value)` uses deterministic, type-tagged local handles, including their generations, instead of
host addresses. The encoding is `0x`, a kind digit (string 1, table 2, function 3, userdata 4, thread 5), then
sixteen hex digits containing a 32-bit generation and a 32-bit slot. Strings with equal bytes share an identity. Nil, booleans and numbers print `NULL`, as in LuaJIT;
width, precision and flags do not affect `%p`. Default `tostring` object text uses the same hex identity;
its type prefix (including the native function's `builtin` label) is preserved.

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

`Limits` is `#[non_exhaustive]`, so that adding a bound (`function_upvalues` is one) breaks no embedder. **Breaking:**
a `Limits { .. }` literal, even with `..Limits::DEFAULT`, no longer compiles outside the crate. Start from
`Limits::DEFAULT` and change what differs, with the `const` methods `with_*` or by assigning fields:

```rust
use luagee::Limits;

const LIMITS: Limits = Limits::DEFAULT.with_memory(64 << 20).with_function_upvalues(120);
let mut limits = LIMITS;
limits.fuel_per_call = 1_000_000;
```

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

### Fuel paid before the work (unreleased fix)

Every place a script sizes the work is paid before the work, in proportion to it, and refused where the fuel left
stops paying: [docs/fuel_audit.md](docs/fuel_audit.md) lists each one, its bound, its price and the test that holds
it. Every arena grows through one bounded reservation ([docs/table_allocation.md](docs/table_allocation.md)). These
prices changed for scripts (fixtures run whole, so each includes the fill or loop around the operation):

| Fixture | Before | After | Why |
|---|---|---|---|
| `t[1]=1` moving 999 keys from the hash part into the array | 5316 | 6315 | each moved key's second lookup, 1 a key |
| `table.maxn` over 1000 live hash keys | 6319 | 5444 | slots looked at, 8 a fuel (was 1 a live key) |
| `table.maxn` over a cleared array of 1000 | 7154 | 7279 | the nil tail it walks, 8 a fuel (was free) |
| `table.maxn` over 1000 removed hash keys | 9323 | 9448 | removed entries it walks, 8 a fuel (were free) |
| `table.foreachi(t, coroutine.running)`, 1000 items | 4153 | 5153 | a native calling a native pays 1 a call |
| `table.foreach(t, coroutine.running)`, 1000 keys | 6323 | 7323 | as above |
| `table.sort(t, rawequal)`, 1000 items | 21321 | 30394 | as above, a call a comparison |
| `string.gsub(s, '.', type)`, 1000 bytes | 11895 | 12895 | as above, a call a match |
| `assert(false, s)` from Lua, 1000-byte `s`, caught | 145 | 269 | locating a message copies and hashes it: 2 a 16 bytes |
| `local t; return t.x` from Lua, caught | 13 | 16 | as above for its 40-byte message, less for naming `t` (below) |
| calling a nil local, caught | 14 | 17 | locating the message, and naming `f` by what the scan looks at |
| arithmetic on a nil local, caught | 13 | 18 | as above |
| indexing a nil field (`t.a.b`), caught | 18 | 21 | as above |
| `xpcall(error, type, 'x')`, 100 times | 1006 | 1106 | the handler `type` is a native called by a native: 1 a call |
| `string.match('y', '[' .. 1000 bytes .. ']')` | 272 | 520 | each scan of a set, 1 a 16 bytes |
| `string.find('y', 'abcdefghijklmnopq')` | 8 | 9 | deciding the pattern is plain, 1 a 16 bytes |
| `string.match('y', '[' .. 1000 bytes)` (never closed), caught | 271 | 333 | the scan that finds it malformed, 1 a 16 bytes |
| an error message of `n` bytes reaching the host | — | + `n / 16` | converting it to text |
| naming the register of an error (`field 'x'`) | `1 + pc/8 + locals/8` | units looked at / 8 | what the scan looks at, the name's copy included (+3 to +5 in the rows above) |
| a function taking an upvalue while it compiles (`load`, a chunk with nested functions) | 0 | 4 | the name is copied and indexed; a name used deep inside nested functions is taken by each of them, however short the source |
| creating a closure of 8, 60 or 255 upvalues (one closure, the loop around it left out) | 5, 5, 5 | 6, 12, 36 | the cells it copies, 1 a 8 |
| `collectgarbage()` over 1000 tables in a table | 10090 | 10299 | roots paid, a removed entry looked at counts |
| `collectgarbage()` after 1100 garbage strings | 10298 | 10259 | a string found in a table's piece is marked there, not traced again |
| `collectgarbage()` 64 frames deep (a tail call each: one frame) | 529 | 492 | marked or empty values no longer count as work |
| `collectgarbage()` 64 frames deep, 5 nil locals a frame | 593 | 844 | every root is paid, nil registers included: 1 a 4 |
| `table.new(4096, 4096)`, `table.new(0, 0)`, `table.clear` of 3 items | 14, 14, 29 | 15, 15, 30 | a collection that starts during the chunk pays for its roots |

A host's own calls follow the same rule ([docs/fuel_audit.md](docs/fuel_audit.md), "Host boundaries"; pinned in
`tests/fuel_prices.rs`): `Lua::call` costs `1 + args / 8` to cross, then the callee. A Lua function pays its
instructions; a native function pays 1 for the call, so calling a native that does no paid work costs 2 and an empty
Lua function (its return) costs 2 as well. `Vm::call` adds the results' `1 + results / 8`.

Unchanged, and pinned (`tests/table_fuel.rs`, `tests/fuel_prices.rs`): dense writes 4147, length appends 6147, fill
and clear 7151, a numeric `for` 1006, `next` and `table.foreach` over empty slots (1 a slot), callbacks written in Lua,
numeric coercion, `load` with a reader (its pieces prepay the compile price), a refused bytecode chunk. The
collector's worklist now counts as memory while it holds values (`collectgarbage("count")` shows it during marking). A call refused for want
of fuel now refuses before work it cannot pay for, so some refusals come earlier than they did; the exact price of a
call still suffices and one less is refused.

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
