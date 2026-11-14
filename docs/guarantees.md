# Guarantees

What a host can rely on when it runs untrusted scripts in luagee: the bounds that keep the host alive, and exactly what a result depends on.
Not covered here: dialect differences ([differences.md](differences.md)) and the host API ([embedding.md](embedding.md)).

## The determinism contract

A call's observable result is a function of these, and of nothing else:

- the **source text** it runs, with the chunk names it is given (error positions print them);
- the **host calls**: which host functions are installed, what they return and do, and the order and timing, in ticks,
  of the host's own calls into the interpreter (`Lua::exec`, `call`, `load`, `begin_tick`, `collect`, `pin`);
- the **`Limits`** the interpreter was made with;
- the **random seed** (`Keys::random`), including every `math.randomseed` the script does;
- the **version of this crate** (and, for the math functions listed in "Numbers" below, the platform).

"Observable result" means the returned values, what `print` receives, error messages and tracebacks, `tostring` and
`%p` identities, table iteration order, `collectgarbage("count")`, and the cost: fuel spent, memory counted
(`Lua::memory`), and the point at which a fuel or memory limit ends the call.

It does **not** depend on:

- **the hash key** (`Keys::hash`). The test `hash_key_independence` runs the same programs under three to six keys and
  requires identical output, outcome, fuel spent and memory counted, also when a fuel limit (at the exact cost minus
  one, half, a third) or a memory limit cuts the run short. The programs are: 7 written for it (string keys with
  insertions, deletions, reinsertion and clearing during traversal; number keys through array/hash migration, floats,
  negatives and huge keys; table, function, coroutine and boolean keys with their `%p` identities; interning and
  collection; error messages; random draws used as keys; metatables and coroutines), the 81 conformance scripts
  and the 161 hostile scripts. Nothing differed. Table lookups pay no fuel for probes and the storage a table holds
  depends on how many keys it has, never on where they hash;
- the **platform**: word size and byte order do not enter (sizes are fixed constants, checked at compile time); the
  same corpora run natively and under `wasm32-wasip1` in the project's gate (`tools/gate.sh`, step `wasm-corpus`).
  The one exception is the math functions named in "Numbers" below;
- **addresses**: `tostring` and `%p` print a kind and an arena slot with its generation, never a host address
  (`tests/pointer_process.rs` compares fresh processes); table iteration does not follow hash order or addresses;
- **time and threads**: the library reads no clock and starts no thread (`tools/wasm_audit.sh` checks that it uses no
  host clock), and an interpreter is single-threaded. The process-wide counter that tells interpreters apart is not
  visible to scripts, and results do not change with how many interpreters run at once (the same test runs eight in parallel);
- **the allocator**: the memory limit and `Lua::memory` count what the interpreter itself asks for, never what the
  allocator returns.

`math.random` is xoshiro256** seeded by SplitMix64 from `Keys::random`; the same seed and calls give the same numbers
on every platform, and `random_stream_is_pinned_for_a_host_seed` pins the first draws for three seeds against an
independent implementation.

### The hash key

`Keys::hash` is the 128-bit key of the SipHash-1-3 that hashes table keys and interned strings. It exists so that a script
cannot choose keys that collide in the index: a lookup walks the colliding entries, and that walk is real time
that fuel does not pay for. So a host facing untrusted scripts keeps the key secret from them and draws a different one
per interpreter from its entropy. Because no result depends on it (above), a host that wants identical reruns may fix it:
doing so changes nothing a script or the host can observe through this API, and it only removes a source of difference
in how long the index walks take. Fixing it is safe only where scripts cannot learn it by timing.

### Not guaranteed

- **Anything the embedder injects.** Its own functions, the clocks or entropy they read, when and how often it
  calls the interpreter and what it passes in are the embedder's; so is the point in wall time at which it gives up on a call.
- **Other versions of this crate.** Fuel prices, memory figures, error wording and `%p` encodings may change between
  versions (the 0.1 changes are listed in [embedding.md](embedding.md)). A recorded run is reproducible with the version that recorded it.
  Different Rust compiler versions are not tested against each other.
- **Different `Limits`.** Limits are inputs: another limit gives another stopping point, and nothing is promised about how
  the costs of two runs under different limits relate.
- **Bit-identical math functions across platforms.** See "Numbers" below.
- **The length of a table with holes**, beyond being a border (see [differences.md](differences.md)), though it is the same one every time.

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
  hash key and `math.random`'s seed are separate, and both come from the host (see "The hash key" above).
- No IO, OS, FFI, JIT or bytecode: `load`, `loadstring` and `dofile` take source text only. There is no
  `string.dump`, `newproxy`, `io`, `os`, `ffi` or `jit`, and `debug` has only `traceback` and a `getinfo` that reports
  a function's source and lines.
- `require` and `dofile` name a module, never a path: `ModuleName` admits only names that stay inside a folder, and
  the host's `Loader` reads it. The `fs` feature adds `FolderLoader`, which reads `<root>/<name>.lua` and refuses
  symlinks, non-regular files and files past a size bound.
- An error inside a script never panics the host, and the interpreter runs the next call.

The hostile corpus (`crates/luagee/tests/hostile`) holds scripts that attack these bounds; each declares on its first
line how it must end, and the tests check that it does.

## Numbers, identities and iteration order

Lua's numbers are IEEE-754 doubles. Floating-point arithmetic happens in the `number` module only: one IEEE
operation per Lua operation, rounded to nearest with ties to even and never fused; numbers are printed and read in
Rust, never through C's `printf` or `strtod`. The exceptions are the functions that IEEE 754 does not define exactly
(`^`, `math.pow`, `exp`, `log`, `log10`, the trigonometric and hyperbolic functions, `atan2`): they all go through one
table in `number::math`, which calls the Rust standard library, hence the platform's math routines. Tables iterate in the
order their keys first arrived (`src/table.rs`), `tostring` of a table or function prints an id (below), the collector
runs at points the interpreter's own accounting decides, and `math.random` draws from a stream the host seeds.

Numeric-for loops preserve LuaJIT's signed-zero direction for real steps. A NaN step always uses the Lua 5.1
nonpositive comparison (`limit <= index`), independent of its sign or payload. This intentionally differs from
LuaJIT where its sign-bit test makes unequal-bound NaN loops depend on how that NaN was produced or the platform.
Equal-bound and NaN-bound cases remain reference-tested; unequal-bound NaN expectations test this explicit rule.

`string.format("%p", value)` uses deterministic, type-tagged local handles, including their generations, instead of
host addresses. The encoding is `0x`, a kind digit (string 1, table 2, function 3, userdata 4, thread 5), then
sixteen hex digits containing a 32-bit generation and a 32-bit slot. Strings with equal bytes share an identity. Nil, booleans and numbers print `NULL`, as in LuaJIT;
width, precision and flags do not affect `%p`. Default `tostring` object text uses the same hex identity;
its type prefix (including the native function's `builtin` label) is preserved.
