# luagee

A sandboxed Lua 5.1 interpreter with LuaJIT 2.1's extensions, in safe Rust, for hosts that must run untrusted scripts
safely and get the same result every time.

The host sets the fuel, memory and size bounds; a script that crosses one ends with an error no script can catch, and
the host keeps running.

```rust
use luagee::{Error, Keys, Limits, Lua};

// Draw both from the host's entropy: scripts must never learn the hash key.
let keys = Keys { hash: [0x1234_5678_9abc_def0, 0x0fed_cba9_8765_4321], random: 42 };
let mut lua = Lua::new(Limits::DEFAULT, keys)?;
let results = lua.exec("squares", b"local t = {} for i = 1, 3 do t[i] = i * i end return table.concat(t, ' ')")?;
assert_eq!(lua.bytes(results[0]), Some(&b"1 4 9"[..]));

// A runaway script runs out of fuel; the interpreter is still usable afterwards.
lua.begin_tick();
assert!(matches!(lua.exec("loop", b"while true do end"), Err(Error::Limit(_))));
```

## What you get

- **Safe Rust, no dependencies**: `#![forbid(unsafe_code)]`; builds for `wasm32-unknown-unknown` and `wasm32-wasip1`.
- **Everything bounded**: fuel for every VM instruction and for the work of every library function, paid before the
  work is done; memory by the interpreter's own count; string length, call depth, syntax and pattern depth, source
  size, native recursion.
- **Hash-flooding resistant**: table keys hash with a keyed SipHash-1-3 whose key only the host knows.
- **No ambient authority**: no IO, OS, FFI, JIT or bytecode; modules load only through the host's `Loader`.
- **Deterministic**: results, errors, iteration order, fuel spent and memory counted depend on the source, the host's
  calls, the limits and the random seed, and not on the platform, addresses, time, threads or the hash key
  (exception: `^`, `exp`, `log` and the trigonometric functions use the platform's math routines).
- **Checked against references**: 81 conformance scripts print what LuaJIT 2.1 prints, byte for byte; 161 hostile
  scripts attack the bounds; the PUC-Rio Lua 5.1 tests (16 files), LuaJIT's test cleanup (378 cases) and Luau's
  Lua-5.1 conformance scripts (15 files) run, with every skip listed with its reason; seven fuzz targets cover the
  parser, the VM, the standard library, the host API, determinism and a differential check.

## What it is not

A Lua 5.1 and LuaJIT-flavoured language, not Lua 5.2+. No JIT, no C API and no `io`, `os` or `ffi`, so it does not
replace PUC Lua or LuaJIT for code that needs them; weak tables and `__gc` have no effect.

## Install and read more

`cargo add luagee`, Rust 1.95 or newer.

- [Guarantees](https://github.com/alexstanovoy/luagee/blob/main/docs/guarantees.md): the safety bounds and the determinism contract.
- [Embedding](https://github.com/alexstanovoy/luagee/blob/main/docs/embedding.md): keys, limits, fuel, handles, userdata.
- [Differences](https://github.com/alexstanovoy/luagee/blob/main/docs/differences.md): from Lua 5.1 and LuaJIT.
- [Fuel audit](https://github.com/alexstanovoy/luagee/blob/main/docs/fuel_audit.md), [development](https://github.com/alexstanovoy/luagee/blob/main/docs/development.md), [changelog](https://github.com/alexstanovoy/luagee/blob/main/crates/luagee/CHANGELOG.md).

Version 0.1: the API may still change before 1.0. MIT licence, see [LICENSE](https://github.com/alexstanovoy/luagee/blob/main/LICENSE).
