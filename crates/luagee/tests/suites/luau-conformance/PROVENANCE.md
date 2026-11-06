# Luau conformance tests

- Source: https://github.com/luau-lang/luau, release tag `0.745` (commit
  `2a3d8937c19c8fd9f7b72b6ec9068c62760e6588`, 2026-10-30), downloaded 2026-11-06 as
  https://codeload.github.com/luau-lang/luau/tar.gz/refs/tags/0.745.
- sha256 of that archive: `53c22773bace6cabbabaead576fbc08d11a0317fce11a3dbf9d027dda6849248` (GitHub generates
  archives, so `SHA256SUMS` lists each file's own checksum as well).
- License: MIT, Copyright (c) 2019-2026 Roblox Corporation and Copyright (c) 1994-2019 Lua.org, PUC-Rio; the
  repository's `LICENSE.txt`, vendored here.

These are the files of the repository's `tests/conformance/` that a LuaJIT 2.1 parser reads, byte for byte
(`SHA256SUMS`); the few that still use Luau's `+=` or `continue` are listed in `../skips.txt`. Left out, as Luau
syntax all through (types, `if` expressions, string interpolation, `//`, `+=`):
basic, bitwise, buffers, classes, coroutine, cyield, debugger, errors, events, explicit_type_instantiations,
ifelseexpr, iflocal, integers, integers_regspill, interrupt, jit_inliner, math, native, native_integer_spills,
native_types, native_userdata, pcall, stringinterp, tables, udata_direct, userdata, utf8, vector, vector_library,
whilelocal.

`tests/suites.rs` runs each file as Luau's harness (`tests/Conformance.test.cpp`) does: a file passes when it returns
"OK". `../skips.txt` lists what does not run as written and why.
