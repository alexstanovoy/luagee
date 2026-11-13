//! The collector's pieces of a large object, with the script mutating the object between two pieces.

use super::{Partial, Phase};
use crate::{GcStress, Keys, Limits, Lua, Value};

/// A table of `{i}` tables at keys 2..=count in its hash part, rooted by a global; marking started and run until the
/// table's hash entries are half traced. Returns the interpreter, stopped there.
fn half_traced(count: usize) -> Lua {
    let mut lua = Lua::new(
        Limits {
            fuel_per_call: 1 << 40,
            fuel_per_tick: 1 << 40,
            ..Limits::DEFAULT
        },
        Keys {
            hash: [193, 271],
            random: 0,
        },
    )
    .expect("state");
    lua.set_gc_stress(GcStress::Off);
    let source = format!("t = {{}} for i = 2, {count} do t[i] = {{i}} end");
    lua.exec("prepare", source.as_bytes()).expect("prepares");
    lua.collect().expect("collects");
    let vm = lua.vm();
    vm.gc_start().expect("marking starts");
    for _ in 0..1_000_000 {
        if let Some(Partial::Table { entries, .. }) = vm.gc_partial
            && entries < count / 2
            && entries > count / 4
        {
            return lua;
        }
        vm.trace_gray(16).expect("traces");
    }
    panic!("the table is never half traced");
}

/// Runs `source` (marking goes on at its instructions), finishes the collection, and checks every `t[i][1] == i`.
fn mutate_then_check(mut lua: Lua, count: usize, source: &str) {
    assert_eq!(lua.vm().heap.phase, Phase::Marking);
    lua.exec("mutate", source.as_bytes()).expect("mutates");
    lua.collect().expect("collects");
    let check = format!("for i = 2, {count} do assert(t[i][1] == i, i) end");
    lua.exec("check", check.as_bytes())
        .expect("every value is alive");
    assert_eq!(lua.vm().heap.phase, Phase::Idle);
    let _ = Value::Nil;
}

#[test]
fn a_migration_into_the_array_while_the_hash_is_half_traced_keeps_the_moved_values() {
    let count = 40_000;
    let lua = half_traced(count);
    // t[1] moves every t[2..] from the hash part to the array part, entries not traced yet included.
    mutate_then_check(lua, count, "t[1] = {1}");
}

#[test]
fn a_hash_rebuild_while_the_hash_is_half_traced_keeps_the_moved_entries() {
    let count = 40_000;
    let lua = half_traced(count);
    // Removing keys, then adding enough new ones, rebuilds the hash part: live entries move down.
    mutate_then_check(
        lua,
        count,
        "for i = 1, 5000 do t[-i] = i end for i = 1, 5000 do t[-i] = nil end for i = 5001, 60000 do t[-i] = i end",
    );
}

/// A coroutine traced before it runs, then moving old objects onto its stack (from a table not traced yet, which then
/// forgets them) and yielding: it is traced again, and the objects survive. The table is large, and its array traced
/// from the end, so that its first items are not reached before the coroutine moves them.
#[test]
fn a_coroutine_traced_before_it_runs_is_traced_again_when_it_stops() {
    let mut lua = Lua::new(
        Limits {
            fuel_per_call: 1 << 40,
            fuel_per_tick: 1 << 40,
            ..Limits::DEFAULT
        },
        Keys {
            hash: [193, 271],
            random: 0,
        },
    )
    .expect("state");
    let co = lua
        .exec(
            "prepare",
            b"t = {} for i = 1, 100000 do t[i] = {i} end
              co = coroutine.create(function()
                  coroutine.yield()
                  local a, b, c = t[1], t[2], t[3]
                  t[1], t[2], t[3] = nil, nil, nil
                  coroutine.yield()
                  assert(a[1] == 1 and b[1] == 2 and c[1] == 3)
              end)
              coroutine.resume(co)
              return co",
        )
        .expect("prepares")[0];
    lua.collect().expect("collects");
    let vm = lua.vm();
    vm.gc_start().expect("marking starts");
    vm.trace(co);
    while let Some(Partial::Thread { .. }) = vm.gc_partial {
        vm.trace_gray(1).expect("traces the coroutine");
    }
    assert_eq!(vm.heap.phase, Phase::Marking);
    lua.exec("move", b"assert(coroutine.resume(co))")
        .expect("moves the objects");
    lua.collect().expect("collects");
    lua.exec("check", b"assert(coroutine.resume(co))")
        .expect("the moved objects are alive");
}
