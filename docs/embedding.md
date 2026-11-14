# Embedding luagee

How a host creates an interpreter, bounds it, calls it, and lends it functions and data: keys, limits, fuel, handles, pinning, userdata.
The fuel prices that changed during 0.1 are in the table below; every price is audited in [fuel_audit.md](fuel_audit.md).

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

`Error`, `Limit`, `LimitsError` and `ModuleNameError` are `#[non_exhaustive]`: match them with a wildcard arm, and treat
an error you do not know as a failed call (a script's call that ended in an `Error` has failed, whatever the kind). `Value`
(the eight types of Lua), `LuaError` (a catchable error or an uncatchable limit) and `GcStress` (a set of modes) are
closed on purpose and stay exhaustive.

`Lua::new` refuses limits that cannot hold an interpreter (`Error::InvalidLimits`, naming the field): a `memory` below
`Limits::MIN_MEMORY` (64 KiB) and a `string_length` below `Limits::MIN_STRING_LENGTH` (16). Every other value is
accepted and a script meets it as its own bound; `Limits::validate` checks without making an interpreter.

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
stops paying: [fuel_audit.md](fuel_audit.md) lists each one, its bound, its price and the test that holds
it. Every arena grows through one bounded reservation ([table_allocation.md](table_allocation.md)). These
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
| `load`, `loadstring`, `require`, `dofile` of 1,276 bytes of ordinary code (`ORDINARY` in `tests/fuel_prices.rs`, 4 times) | 319 | 2,636 | every token read costs 8 (`TOKEN_FUEL`), after the quarter fuel a byte |
| the same of 114,840 bytes (360 times) | 28,710 | 236,083 | as above: about 8 times, 2 fuel a byte; at the default `fuel_per_call` a call loads some 240 KB of such code (some 180 KB of the denser conformance scripts), so a host with larger modules raises it or loads them with `Lua::load`, which is outside script fuel |
| the whole chunk `load(string.rep(unit, 1000))` of `--x\n` (comments), `a=a ` and `x={1,2,3}\n` | 1,513, 1,513, 3,763 | 1,521, 25,521, 75,771 | as above: a comment costs its bytes only, a token 8; these include building the source with `string.rep` |
| a function taking an upvalue while it compiles (`load`, a chunk with nested functions) | 0 | 4 | the name is copied and indexed; a name used deep inside nested functions is taken by each of them, however short the source |
| creating a closure of 8, 60 or 255 upvalues (one closure, the loop around it left out) | 5, 5, 5 | 6, 12, 36 | the cells it copies, 1 a 8 |
| `collectgarbage()` over 1000 tables in a table | 10090 | 10299 | roots paid, a removed entry looked at counts |
| `collectgarbage()` after 1100 garbage strings | 10298 | 10259 | a string found in a table's piece is marked there, not traced again |
| `collectgarbage()` 64 frames deep (a tail call each: one frame) | 529 | 492 | marked or empty values no longer count as work |
| `collectgarbage()` 64 frames deep, 5 nil locals a frame | 593 | 844 | every root is paid, nil registers included: 1 a 4 |
| `table.new(4096, 4096)`, `table.new(0, 0)`, `table.clear` of 3 items | 14, 14, 29 | 15, 15, 30 | a collection that starts during the chunk pays for its roots |

A host's own calls follow the same rule ([fuel_audit.md](fuel_audit.md), "Host boundaries"; pinned in
`tests/fuel_prices.rs`): `Lua::call` costs `1 + args / 8` to cross, then the callee. A Lua function pays its
instructions; a native function pays 1 for the call, so calling a native that does no paid work costs 2 and an empty
Lua function (its return) costs 2 as well. `Vm::call` adds the results' `1 + results / 8`.

Unchanged, and pinned (`tests/table_fuel.rs`, `tests/fuel_prices.rs`): dense writes 4147, length appends 6147, fill
and clear 7151, a numeric `for` 1006, `next` and `table.foreach` over empty slots (1 a slot), callbacks written in Lua,
numeric coercion, a refused bytecode chunk. (A reader's pieces still prepay the quarter fuel a byte of the compile
price; its tokens are paid as the lexer reads them, as for any source.) The
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
