# Changelog

## 0.1.0

The first release of luagee: a Lua 5.1 interpreter with LuaJIT 2.1's extensions (`goto`, the `bit` library, number
syntax, library behaviour and error wording), in safe Rust, with no dependencies, for running scripts nobody vouches for.
It runs under WebAssembly (`wasm32-unknown-unknown`, `wasm32-wasip1`) on one thread and wasm's default stacks.

- **Limits.** The host gives every interpreter `Limits` (non-exhaustive: start from `Limits::DEFAULT`): fuel per call and
  per tick, memory by the interpreter's own count, string length, call depth, syntax and pattern depth, the upvalues a
  function may capture (60 as in Lua 5.1 and LuaJIT, at most 255), and the size and number of modules. Running out of fuel
  or memory, or of source, ends the call with `Error::Limit`, which no script can catch. `Lua::new` refuses limits that
  cannot hold an interpreter (`Error::InvalidLimits`).
- **Fuel.** Every VM instruction costs a fuel and every library function pays for its work, before it does it: bytes
  copied, pattern steps, values moved, allocations, the collector's walks, and compilation, which costs a quarter fuel a
  source byte and 8 fuel a token as the lexer reads it, and 4 fuel for each upvalue a function takes. The prices and the
  measurements behind them are in `docs/fuel_audit.md` of the repository.
- **Host boundaries.** Host functions are trusted Rust; `Lua::call`, `Vm::call`, handles, pinning and userdata are
  described in the README's embedding section. `Error`, `Limit`, `LimitsError` and `ModuleNameError` are
  `#[non_exhaustive]`: match them with a wildcard arm that treats an unknown error as a failed call.
- **Determinism.** The same keys and the same calls give the same output on every platform; the hash key and
  `math.random`'s seed come from the host.
- **Differences from LuaJIT** are listed in the README.
