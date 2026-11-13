# Fuel and memory audit

Every place where a script decides how much work the interpreter does or how much it allocates, what bounds it, where
it is paid and where its memory is checked. The rule each row is held to: the work is paid **before** it is done (or
is a bounded constant), a call refused for want of fuel has done no more than its fuel paid for, and memory is counted
**before** it is allocated.

Two kinds of evidence back the rows:

- `src/work/tests.rs` (`script_sized_work_is_paid_in_proportion_and_before_it_is_done`): test-only work counters at
  the loops that do the work (`src/work.rs`), at three sizes (64, 1024 and 16384). For every row: the counted work,
  priced as the operation prices it, stays within the fuel spent; a call given half that fuel is refused having done
  no more work than the half pays for, and leaves the state as it was; the exact fuel suffices and one less is refused.
  The counters are deterministic: no clock is involved.
- Measurements of nanoseconds per fuel at three sizes (1024, 16384 and 262144 unless noted; finite prepared calls, a
  release build, a busy machine, so read them to a factor of about two). A row whose cost a fuel stays flat across the
  sizes is paid in proportion; the columns give the value before (the base this work started from) and after.

"Constant" means the work does not depend on anything a script sizes (or on a bound fixed by `Limits`), so there is
nothing to grow.

## Rows that were wrong, and are fixed

| Operation | Bound on the work | Paid before | ns per fuel before | after | Test |
|---|---|---|---|---|---|
| `table.maxn`, cleared array | retained array slots | slots looked at, 8 a fuel, scan stops where fuel stops | 86, 861, 15978 | 10, 6, 5 | work rows `table.maxn, *` |
| `table.maxn`, removed hash keys | retained hash entries | as above | 168, 2615, 44670 | 3, 3, 5 | work rows |
| `table.foreachi` with a native callback | `#t` calls | each native-to-native call 1 fuel | 5686, 87108, 1305246 | 34, 34, 36 | work row `table.foreachi, native callback` |
| `table.foreach` with a native callback | entries | as above (and 1 an entry, as before) | 64, 47, 58 | 32, 33, 33 | work row |
| `table.sort` with a native comparator | comparisons | as above | 29, 30, 29 | 19, 20, 20 | work row |
| `next`, `table.foreach` over empty slots | retained slots | 1 a slot stepped over, walk stops where fuel stops (price unchanged) | flat, but refused only after walking all | flat | work rows `next, *`, `table.foreach, removed keys` |
| `string.rep` unused separator | separator length | no copy any more | 18, 32, 248 | 24, 23, 23 | measurement |
| `table.concat` unused separator | separator length | no copy any more | 23, 34, 221 | 25, 25, 26 | measurement |
| `collectgarbage` invalid option | option length | no copy any more | 104, 120, 511 | 104, 92, 100 | measurement |
| `assert(false, s)` | message length | no copy (the same string is raised) | 57, 367, 5674 | 30, 30, 30 | measurement, work row `assert, long message, Lua caller` |
| error located by a Lua frame | message length | copy and hash, 2 fuel a 16 bytes | 67, 402, 6487 (assert from Lua) | 5, 2, 1 | work rows `assert, ...`, `error, ...` |
| error message going to the host | message length | conversion, 1 fuel a 16 bytes; ends by fuel when unpaid | 95, 1084, 15648 | 11, 9, 8 | `an_error_message_exported_to_the_host_is_paid_before_it_is_converted` |
| pattern set `[...]`, finding its `]` | set length | 1 fuel a 16 bytes, scan stops where fuel stops | 188, 3811, 43608 | 8, 7, 6 | work rows `string.match, long set*`, `a_long_unclosed_set_is_malformed_only_when_its_scan_is_paid` |
| pattern set tested at each position | set length × positions | 1 fuel a 16 bytes of set at each test | 25, 345, 4517 (sets of 16, 256, 4096) | 10, 9, 9 | work row `string.find, long set at each position` |
| `string.find` deciding a pattern is plain | pattern length | 1 fuel a 16 bytes | constant fuel | flat | work row `string.find, long plain-looking pattern` |
| numeric string coercion (arithmetic, `tonumber`, `check_number`, `for`) | string length | 1 + 1 a 16 bytes, refused before parsing (was taken without checking, then parsed) | flat, but a tail-called `tonumber` succeeded on half its price | flat | work rows `tonumber, ...`, `arithmetic, ...` |
| error naming (`local 'x'`, `field 'y'`) | code before the error, locals at each register followed, name length | 1 unit a step, 8 a fuel, scan stops where fuel stops; name copy counted | flat, but copy and scan unpaid past an estimate | flat | work row `error naming, long field name` |
| collector roots | running stack, pins, frames | 1 fuel per 4 roots, nil registers included | 55, 349, 9395 (16, 128, 1024 frames of 180 nils) | 4, 2, 2 | work row `collectgarbage, deep nil registers` |
| collector, one large table or coroutine stack | object size | traced in pieces, each paid before, nothing past the fuel | 4225 fuel of work before refusing at 2147 (16384 slots) | within the fuel | work rows `collectgarbage, one large *`, `vm::tests::*` |
| collector, refusing at exact fuel | — | a marked or empty value costs nothing and is never refused | exact fuel refused | exact fuel suffices | work row `collectgarbage, live tables` |
| coroutine switch while marking | coroutine stack | traced again (in pieces, paid) instead of copied | unpaid copy at each switch | paid | `vm::tests::a_coroutine_traced_before_it_runs_is_traced_again_when_it_stops` |
| `coroutine.resume` of a deep coroutine | frames | the boundary frame is known, no walk | 27, 31, 69 (16, 128, 1024 frames) | 25, 20, 28 | measurement |
| hash-to-array migration | moved keys | second lookup, 1 a key | 13, 14, 32 (whole fill + move) | 13, 17, 23 | work row `a write moving hash keys into the array`, `table_fuel` pin |
| function, cell, userdata, string, coroutine arenas | arena size | relocation of a capped growth, before reserving | false memory errors in bands of the cap | none | `garbage_functions_cells_and_coroutines_exhaust_fuel_without_false_memory_errors` |
| `loadstring(s)`, `load(s)` refused (bytecode) | source length | no copy any more: constant | 17000 ns a fuel at 4 MB, caught and repeated | constant fuel at any length | `a_refused_bytecode_chunk_costs_the_same_at_any_length` |
| `load(reader)` pieces | source length, at most `module_size` | each piece at the compile price (1 a 4 bytes) before it is copied; the compile itself pays for the upvalues it takes (below) | 1150 ns a fuel at 1 MB | within the fuel | work row `load, a reader's piece then a number` |
| pattern set that never closes | pattern length | the scan to the pattern's end, 1 a 16 bytes, before the error | 190000 ns a fuel at 4 MB, caught and repeated | paid | `a_long_unclosed_set_is_malformed_only_when_its_scan_is_paid` |
| a sweep stopped by fuel at an arena's end | — | each sweep step and the string index's shrink paid before; the arena is an enum ending in `Done` | the next call panicked ("six arenas") | resumes | `*_collection_stopped_by_fuel_anywhere_resumes_on_the_next_call` (plain, incremental, full) |
| compile, a token | tokens, each lexed, parsed and compiled: 5 to 300 ns whatever its length | `TOKEN_FUEL` (8) as the lexer reads it, after the quarter fuel a byte up front; a compile the call cannot pay for stops at the token it cannot pay | 80 to 400 (`examples/compile_price.rs`: one-line statements 250, operator chains 350 to 430, field chains 600, distinct globals 200) | at most 32 (nested tables 20 to 32, distinct globals 21 to 24, distinct strings 14 to 24, typical code 10 to 17) | work rows `load, reads under 60 upvalues`, `load, reads under 40 nested functions of 150 locals`, `load, a block of labels`, `a_compile_whose_fuel_ends_stops_reading_tokens_where_it_ends`, prices in `tests/fuel_prices.rs` |
| compile, a name use (`resolve`) | functions nested x locals and upvalues scanned for each use | no scan: an index of the active locals by name finds the innermost declaration in one lookup; the upvalues a use makes each function take are paid when taken (`CAPTURE_FUEL`, 4) | 3338 (40 functions of 150 locals nested), 516 (255 upvalues) | 231 and 215 with the quarter fuel a byte alone; 14 and 22 with the token price above | work rows `load, reads under 60 upvalues`, `load, reads under 40 nested functions of 150 locals`, `upvalues_taken_through_nested_functions_are_paid_one_by_one_before_they_are_taken` |
| compile, a block of labels | statements after each, to see whether only labels follow | looked for once from the block's end | 8000 (40000 labels) | 290 | work row `load, a block of labels` |
| creating a closure | upvalues copied, at most `Limits::function_upvalues` (default 60, at most 255) | `ALLOCATION_FUEL` + 1 a 8 upvalues, before the copy (the cells are counted as memory before the function's storage is built, `Heap::new_lua_function`) | 19, 49 (60, 255 upvalues) | 8, 12 | `a_closure_costs_its_instruction_an_allocation_and_one_fuel_for_every_eight_upvalues_it_copies`, `a_closure_whose_copy_is_not_paid_is_refused_before_it_is_made`, work row `closures of 56 upvalues`, `a_closure_the_call_cannot_pay_for_copies_no_cells` (the order: no cell is copied before the price is paid) |
| `table.new(narray, nhash)` storage | hints up to 4096 | memory counted before the storage is built | built, then refused | refused first | `table_new_is_refused_by_the_memory_cap_before_allocating_its_storage` |

## Rows that hold

### Table library and table access

| Operation | Bound on the work | Paid before | Memory before | ns per fuel after |
|---|---|---|---|---|
| `t[k]`, `t[k] = v`, `rawget`, `rawset` | keyed SipHash with a secret key, index at most half full: expected O(1) probes | 1 an instruction; writes pay copying, migration and rebuilds (`Table::set`) | growth charged through the write budget before reserving | 10, 11, 15 (existing keys) |
| `__index` / `__newindex` chains | 100 links | 1 a link (an instruction's lookups) | — | 17, 17, 17 / 36, 34, 32 |
| `#t`, `table.getn` | binary search over the array part: at most 64 probes | constant | — | constant |
| `table.insert`, `table.remove`, `table.move` | elements moved | 1 a move, before moving | through the write path | 4, 4, 4 / 6, 6, 6 |
| `table.concat` | pieces and their bytes | each piece and its length before copying | string length limit | 8, 8, 8 |
| `table.sort` | comparisons, O(n log n) | 1 a comparison (plus the comparator) | the range stack is O(log n) | 8, 7, 7 |
| `table.clear` | retained slots | 1 a 8 slots | — | 2, 1, 2 |
| `unpack`, `select`, varargs, `{...}` | values moved | 1 a 8 values | stack growth counted | 3, 3, 3 / 5, 5, 5 |
| `pairs`, `ipairs` loops | an entry a step | 1 an instruction + `next` | — | 16, 16, 17 / 13, 12, 12 |
| table constructor `{a, b, ...}` | values | 1 a 8 values (SETLIST) | table storage counted first | 4, 4, 4 |

### Strings

| Operation | Bound on the work | Paid before | Memory before | ns per fuel after |
|---|---|---|---|---|
| `..`, `string.rep`, `sub`, `upper`, `lower`, `reverse`, `byte`, `char`, `format('%s')` | output or input bytes | 1 a 16 bytes before copying | string length limit, then the heap | 2, 1, 2 / 22, 21, 25 / 1–6 |
| `string.format('%q')` | escaped output | expansion before copying | string length limit | 29, 27, 27 |
| other `format` items | field width at most 99 | constant | — | constant |
| `string.find(s, p, i, true)` | source and needle | 1 a 16 bytes before searching (linear search) | — | 27, 26, 26 |
| `find`, `match`, `gmatch`, `gsub` with patterns | match steps; recursion at most `pattern_depth`; 32 captures | 1 a step, back-references and balances before comparing, sets as above | — | 19, 14, 13 / 13, 8, 9 / 6, 4, 6 |
| `gsub` replacement | replacement bytes per match | before appending | string length limit | 13, 11, 10 |
| string comparison, equality | interned: equality is identity; `<` compares bytes, paid | 1 a 16 bytes | — | 2, 0.2, 0.2 |
| `#s`, `tostring(s)` | constant | — | — | constant |
| interning a new string | its bytes | `ALLOCATION_FUEL` + 1 a 16 bytes (`Vm::new_string`) | reserved through `Arena::reserve` | — |

### Base library, errors, loading

| Operation | Bound on the work | Paid before | Memory before | ns per fuel after |
|---|---|---|---|---|
| `error(v, level)` | message bytes; the level walk stops at a boundary or `pcall` frame, and the frames it walks are unwound by the error | message copy and new string, before copying | — | 32, 32, 32 (no position) / 5, 2, 1 (with one) |
| `pcall`, `xpcall` | arguments and results | 1 a 8 values; a native handler 1 a call | stack counted | 5, 4, 4 |
| `loadstring`, `load` | source bytes, at most `module_size`; tokens; upvalues taken, at most functions nested x `Limits::function_upvalues` | 1 a 4 bytes before parsing, `TOKEN_FUEL` (8) a token as the lexer reads it, and `CAPTURE_FUEL` (4) when a function takes an upvalue, each before it is done; a string source is shared, not copied; a reader's pieces are paid before they are copied | source limits, then the heap | 6, 3, 3 |
| `require`, `dofile` | module source through the host's loader (trusted Rust), at most `module_size` | as `load` | module count and size limits | — |
| `tonumber(s, base)` | digits | 1 + 1 a 16 bytes | — | 8, 6, 6 |
| `print` | bytes written | 1 a 16 bytes | — | 3, 0.4, 0.3 |
| `select('#', ...)`, `type`, `rawequal`, `getmetatable`, `setmetatable`, `getfenv`, `setfenv`, `gcinfo`, `collectgarbage('count')` | constant (`getfenv(level)` indexes the frames directly) | — | — | constant |
| `math.*`, `bit.*` | constant, or arguments for `max`, `min`, `band`, `bor`, `bxor` | 1 a 8 arguments | — | 5, 5, 6 |
| `debug.traceback` | at most 22 frames shown, each name paid | before rendering | — | 11, 11, 12 (16, 128, 1024 frames) |
| `debug.getinfo` | constant | — | its table through `Arena::reserve` | constant |

### Coroutines

| Operation | Bound on the work | Paid before | Memory before | ns per fuel after |
|---|---|---|---|---|
| `coroutine.create`, `wrap` | constant | `ALLOCATION_FUEL` | `Arena::reserve` | — |
| `coroutine.resume`, `yield`, a wrapped call | values passed | 1 a 8 values | stack counted | 5, 5, 7 |
| a dead coroutine's frames | frames | its calls paid for them | — | — |

### Collector

| Step | Bound on the work | Paid before | ns per fuel after |
|---|---|---|---|
| roots | running stack, pins, frames, fixed names | 1 a 4 roots | 4, 2, 2 |
| marking an object | a string, cell, userdata: 1; a function: upvalues (at most `Limits::function_upvalues`, 255) and, once a collection, its prototypes' constants (compiled code, paid when loaded) | 1 a 4 units, nothing started past the fuel | 8, 2, 1 (one large table) |
| a table or coroutine stack | its slots, in pieces of up to 4096 | each piece before it is traced | 11, 15, 28 (a hash of tables) |
| sweeping | slots | 1 a 4 slots, each step paid before it is walked; an arena ends with its last slot | 4, 4, 5 |
| the worklist (gray values) | values grayed by paid stores and traces | its growth counted as memory when it grows, and at the end of a collection | — |
| freeing a string | its run in the string index: expected O(1) (secret hash, index at most half full) | with its slot | — |
| freeing a function's last reference to compiled code | the prototype's size | paid when it was compiled | — |
| shrinking the string index | the index | paid before rebuilding, out of the same run as the sweep step, else left for later | — |

### Host boundaries

| Crossing | Bound | Paid before |
|---|---|---|
| `Lua::call` arguments | count | 1 + 1 a 8 values |
| `Lua::call` of a native function | the call itself | 1, on top of the arguments: 2 + 1 a 8 values for a function that does no paid work. A Lua callee pays its instructions instead (an empty one, its return, 1), so an empty native and an empty Lua function cost the same, and no call is free |
| `Lua::call` results | count | the returning instruction, 1 a 8 values |
| an error message to the host | message bytes | 1 a 16 bytes, ending by fuel when unpaid |
| `Vm::call` from a host callback | arguments and results | 1 + 1 a 8 values each way; a native callee also pays 1 for the call, as a native called by a native does |
| host callbacks themselves | arbitrary Rust | trusted: callbacks charge their own work (`Vm::charge_fuel`) |
| `Lua::load`, `pin`, `string`, `set_global`, `collect` | host setup | trusted, outside script fuel (as before) |

## Known limits of this audit

- Compiling is paid per token, by the worst source's cost (`TOKEN_FUEL`), so source that is cheap to read is
  overcharged: a comment or a string is only the quarter fuel a byte it is read at, which makes that 1 to 2 ns a fuel,
  and a run of empty statements, a token of 1 byte each, about 1.3 (against the ~35 of the dearest). Typical code is
  paid at 10 to 17 ns a fuel.
- Hash probing (tables and the string index) is not priced per probe. With a secret SipHash key a script cannot
  choose colliding keys, and every index is kept at most half full, so probe runs are short in expectation; the
  measurements of hash writes stay within a factor of three of array writes at 262144 keys.
- Some work is bounded by a limit rather than paid in proportion: `#t` (64 probes), `__index` chains (100 links),
  `string.format` widths (99), the frames of one coroutine (`call_depth`).
- Times per fuel differ between operations by about an order of magnitude (from under 1 to some 35 ns): the prices
  are proportional, not equal.
