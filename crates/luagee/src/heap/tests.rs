use std::rc::Rc;

use super::{Arena, Heap, TABLE_SIZE};
use crate::{Error, Keys, Limit, Limits, Lua};

fn four_slot_heap(cap: usize) -> Heap {
    let mut heap = Heap::new(cap, [1, 2], 1);
    for _ in 0..4 {
        heap.new_table(0, 0, &mut |_| {
            panic!("geometric growth has no fallback charge")
        })
        .expect("four initial tables fit");
    }
    assert_eq!(heap.tables.slots.len(), 4);
    assert_eq!(heap.tables.slots.capacity(), 4);
    heap
}

#[test]
fn capped_table_arena_growth_prepays_its_exact_relocation_budget() {
    let price =
        1 + (4 * TABLE_SIZE / (crate::value::VALUE_BYTES * crate::vm::VALUES_PER_FUEL)) as u64;
    for initial in [price - 1, price] {
        let mut heap = four_slot_heap(9 * TABLE_SIZE);
        let before = heap.memory;
        let mut left = initial;
        let result = heap.new_table(0, 0, &mut |work| {
            let remaining = left.checked_sub(work);
            left = remaining.unwrap_or(0);
            remaining
                .map(|_| ())
                .ok_or(crate::LuaError::Limit(Limit::FuelPerCall))
        });
        assert_eq!(left, 0, "exact fallback charge, including refusal");
        if initial == price {
            result.expect("minimum prepaid growth succeeds");
            assert_eq!(heap.tables.slots.capacity(), 6);
            assert_eq!(heap.memory, before + 3 * TABLE_SIZE);
        } else {
            assert!(matches!(
                result,
                Err(crate::LuaError::Limit(Limit::FuelPerCall))
            ));
            assert_eq!(
                heap.memory, before,
                "refused work cannot allocate or charge memory"
            );
            assert_eq!(heap.tables.slots.capacity(), 4);
            assert_eq!(heap.tables.slots.len(), 4);
        }
    }
}

#[test]
fn table_arena_refuses_before_work_when_no_new_slot_fits() {
    let mut heap = four_slot_heap(TABLE_SIZE * 38 / 5);
    let before = heap.memory;
    let mut charged = 0;
    let result = heap.new_table(0, 0, &mut |work| {
        charged += work;
        Ok(())
    });
    assert!(matches!(result, Err(crate::LuaError::Limit(Limit::Memory))));
    assert_eq!(charged, 0, "zero-growth refusal precedes relocation work");
    assert_eq!(heap.memory, before);
    assert_eq!(heap.tables.slots.capacity(), 4);
    assert_eq!(heap.tables.slots.len(), 4);
}

fn arena_caps() -> [usize; 3] {
    // Preserve the arena-slot pressure of the original 7/14/28 MiB regressions as the header changes.
    [7, 14, 28].map(|mib| ((mib << 20) / 104) * TABLE_SIZE)
}

fn arena_interpreter(memory: usize) -> Lua {
    Lua::new(
        Limits {
            memory,
            fuel_per_call: 5_000_000,
            fuel_per_tick: 5_000_000,
            ..Limits::DEFAULT
        },
        Keys {
            hash: [193, 271],
            random: 0,
        },
    )
    .expect("state")
}

#[test]
fn garbage_tables_exhaust_fuel_without_false_memory_errors_at_arena_growth_boundaries() {
    let mut failures = Vec::new();
    for memory in arena_caps() {
        let mut lua = arena_interpreter(memory);
        let result = lua.exec("garbage", b"while true do local t={} end");
        if !matches!(result, Err(Error::Limit(Limit::FuelPerCall))) {
            failures.push(format!("memory={memory}: {result:?}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn native_and_host_table_creators_share_the_same_arena_reservation_guard() {
    for memory in arena_caps() {
        for source in [
            &b"local new=require('table.new'); while true do local t=new(0,0) end"[..],
            &b"while true do local t=debug.getinfo(1,'S') end"[..],
            &b"while true do make_table() end"[..],
        ] {
            let mut lua = arena_interpreter(memory);
            let function = lua
                .function(
                    "make_table",
                    Rc::new(|vm, _| {
                        vm.new_table()?;
                        vm.results(&[])
                    }),
                )
                .expect("callback");
            lua.set_global("make_table", function).expect("install");
            let result = lua.exec("garbage", source);
            assert!(
                matches!(result, Err(Error::Limit(Limit::FuelPerCall))),
                "memory={memory}, source={}: {result:?}",
                String::from_utf8_lossy(source)
            );
        }
    }
}

#[test]
fn host_allocation_keeps_unpinned_temporaries_live_without_collecting() {
    let mut lua = arena_interpreter(arena_caps()[0]);
    let first;
    {
        let vm = lua.vm();
        first = vm.new_table().expect("first table");
        let mut refused = false;
        for _ in 0..100_000 {
            if vm.new_table().is_err() {
                refused = true;
                break;
            }
        }
        assert!(
            refused,
            "bounded setup refuses instead of allocating forever"
        );
    }
    assert!(
        lua.vm().raw_get(first, crate::Value::Number(1.0)).is_ok(),
        "allocation alone did not collect the unpinned temporary"
    );
}

std::thread_local! {
    static NUMERIC_HASHES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

pub(super) fn note_numeric_hash() {
    NUMERIC_HASHES.with(|count| count.set(count.get() + 1));
}

#[test]
fn vm_dense_numeric_writes_do_not_hash_array_keys() {
    let mut lua = crate::Lua::new(
        crate::Limits::DEFAULT,
        crate::Keys {
            hash: [193, 271],
            random: 0,
        },
    )
    .expect("state");
    let function = lua
        .load("array", b"local t={} for i=1,1000 do t[i]=i end")
        .expect("compile");
    NUMERIC_HASHES.with(|count| count.set(0));
    lua.call(function, &[]).expect("dense writes");
    assert_eq!(
        NUMERIC_HASHES.with(std::cell::Cell::get),
        0,
        "the production heap write path defers numeric hashing"
    );
}

#[test]
fn exhausted_generations_retire_the_slot_instead_of_reusing_old_handles() {
    let mut arena = Arena::default();
    let original = arena.insert(17);
    arena.begin_collection();
    arena.sweep(usize::MAX, |_| 0, |_, _| {});
    arena.end_sweep(0);
    arena.slots[original.index() as usize].generation = u32::MAX - 1;
    let last = arena.insert(29);
    assert_eq!(last.generation(), u32::MAX);
    arena.begin_collection();
    arena.sweep(usize::MAX, |_| 0, |_, _| {});
    arena.end_sweep(0);
    let replacement = arena.insert(41);
    assert_ne!(last.index(), replacement.index());
    assert!(arena.try_get(last).is_none());
}

#[test]
fn vm_hash_storage_writes_keep_the_numeric_hash_counter_connected() {
    let mut lua = crate::Lua::new(
        crate::Limits::DEFAULT,
        crate::Keys {
            hash: [193, 271],
            random: 0,
        },
    )
    .expect("state");
    let function = lua
        .load("hash", b"local t={} for i=1,1000 do t[-i]=i end")
        .expect("compile");
    NUMERIC_HASHES.with(|count| count.set(0));
    lua.call(function, &[]).expect("hash writes");
    assert_eq!(
        NUMERIC_HASHES.with(std::cell::Cell::get),
        1000,
        "a disconnected counter cannot make the no-hashing test pass vacuously"
    );
}
