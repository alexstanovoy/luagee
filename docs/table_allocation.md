# Arena reservations

Every arena (strings, tables, functions, cells, userdata and coroutines) reserves room for a new object through one
helper, `Arena::reserve`, with the rules below; tables are the example. The fuel it spends comes from the heap's own
copy of the running call's fuel, so a reservation pays before it relocates, wherever it is called from.

Table allocation does not collect. This preserves unpinned host temporaries until
explicit collection or Lua reentry. Each creator uses the same guarded heap
reservation, regardless of whether it runs in a bytecode instruction or a native
callback.

The usual reservation doubles the arena. If that would exceed the temporary
memory ceiling, reserve only the available slot headroom. This bridges to the next
normal collection point instead of reporting a false memory failure before that
point. Refuse when even one new slot cannot fit. Retained capacity is charged
before reservation; an allocator over-grant is also accounted before return.

A capped reservation may not retain geometric amortisation. Its possible
relocation is therefore charged before reserving: `1 + old_slots * slot_size /
(VALUE_BYTES * VALUES_PER_FUEL)`, where the slot size is the arena's counted object size (`TABLE_SIZE` for tables). Ordinary geometric reservations retain their
existing prices. Construction of the initial VM tables is trusted setup, outside
script fuel; subsequent setup already runs with its own setup budget.

Creators covered by the central reservation:

- initial globals and loaded-module registry;
- library tables, package/preload tables and the shared string metatable;
- bytecode table constructors;
- `table.new` and `debug.getinfo`;
- module namespace tables and `package.seeall` metatables;
- host callbacks through `Vm::new_table`.

Functions, cells and coroutines had the same false memory errors, inside the bands where an uncapped doubling did not
fit (functions near 3, 5.5, 11 and 22 MiB, coroutines near 2.25, 4.5, 9 and 18 MiB, closures with a cell near 2 MiB);
`garbage_functions_cells_and_coroutines_exhaust_fuel_without_false_memory_errors` runs caps inside each band.

The 7, 14 and 28 MiB tests exercise the same byte caps as the CLI's `LUA_MEMORY`.
Native and host creators have independent churn coverage. A host-allocation test
keeps an unpinned temporary live across allocation refusal, guarding the no-GC
contract. Memory may still be refused when a host retains too much between safe
points; allocation does not reclaim handles that the host may still use.
