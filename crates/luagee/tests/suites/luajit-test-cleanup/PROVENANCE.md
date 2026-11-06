# LuaJIT-test-cleanup

- Source: https://github.com/LuaJIT/LuaJIT-test-cleanup at commit `014708bceb70550a3ab8d539cff14d9085ca9cb8`
  (2016-10-19, the head of `master`), downloaded 2026-11-06 as
  https://codeload.github.com/LuaJIT/LuaJIT-test-cleanup/tar.gz/014708bceb70550a3ab8d539cff14d9085ca9cb8.
- sha256 of that archive: `6003d032d74e48516526a5e81f1e40ea13072395af85c79b63aeffd415756fba` (GitHub generates
  archives, so `SHA256SUMS` lists each file's own checksum as well).
- License: see `LICENSE` (public domain for Mike Pall's tests; contributed for LuaJIT's public test suite).

This folder is the repository's `test/` folder, byte for byte (`SHA256SUMS`), with the repository's top-level
`README` added. Left out: `lib/ffi/`, `opt/sink/ffi*.lua`, `common/ffi_util.inc` and `sysdep/` (the FFI and the
host system), `src/` (C and C++ for FFI tests), `unportable/` (machine-specific), `misc/` (old scripts not yet in
the indexed tree, outside what `test.lua` runs), `test.lua` (the runner, which `tests/suites.rs` does as it does),
and the top-level `bench/` (benchmarks).

`tests/suites.rs` walks the `index` files and splits each file into its cases as `test.lua` does; `../skips.txt`
lists what does not run as written and why, including what the tree's own metadata leaves out.
