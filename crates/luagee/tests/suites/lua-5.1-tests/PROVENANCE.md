# Lua 5.1 test suite (PUC-Rio)

- Source: https://www.lua.org/tests/lua5.1-tests.tar.gz (the official tests for Lua 5.1, dated 2016-01-18 on
  https://www.lua.org/tests/).
- sha256 of the archive: `49e4ca6561f82ea605908c5041ab5fad66ed9930fa0686675bd51b02767f18ad`, as lua.org publishes it.
- Downloaded 2026-11-06.
- License: the Lua license (MIT), which https://www.lua.org/tests/ says covers the suites; text in `LICENSE`.

Every file here is byte for byte the archive's `lua5.1-tests/` member of the same name (`SHA256SUMS`). Left out:
`libs/` (C libraries for `package.loadlib`, a makefile, an empty folder) and `etc/` (`ltests.c`, `ltests.h`: the
internal testC library), which are C, not tests.

`tests/suites.rs` runs each file as `all.lua` does, with the results `all.lua` checks; `../skips.txt` lists what
does not run as written and why.
