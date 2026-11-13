# Table arena reservations

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
relocation is therefore charged before reserving: `1 + old_slots * TABLE_SIZE /
(VALUE_BYTES * VALUES_PER_FUEL)`. Ordinary geometric reservations retain their
existing prices. Construction of the initial VM tables is trusted setup, outside
script fuel; subsequent setup already runs with its own setup budget.

Creators covered by the central reservation:

- initial globals and loaded-module registry;
- library tables, package/preload tables and the shared string metatable;
- bytecode table constructors;
- `table.new` and `debug.getinfo`;
- module namespace tables and `package.seeall` metatables;
- host callbacks through `Vm::new_table`.

The 7, 14 and 28 MiB tests exercise the same byte caps as the CLI's `LUA_MEMORY`.
Native and host creators have independent churn coverage. A host-allocation test
keeps an unpinned temporary live across allocation refusal, guarding the no-GC
contract. Memory may still be refused when a host retains too much between safe
points; allocation does not reclaim handles that the host may still use.
