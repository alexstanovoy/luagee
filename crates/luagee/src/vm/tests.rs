//! The collector: pieces of a large object with the script mutating the object between two pieces, and collections
//! stopped by fuel anywhere and resumed by the next call.

use super::{Partial, Phase};
use crate::{Error, GcStress, Keys, Limit, Limits, Lua, Value};

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

const AMPLE: u64 = 1 << 40;

/// A script that makes garbage of every kind (enough strings that the string index grows and then shrinks), keeps
/// some, collects, and keeps more; and a check of what it keeps, true for any prefix of the script that ran.
const COLLECTED: &str = "
    live = {}
    for i = 1, 1100 do local s = 'k' .. i end
    for i = 1, 40 do live[i] = {i, tostring(i)} end
    local cell = 0
    for i = 1, 60 do local f = function() cell = cell + i return cell end f() local t = {f} end
    local co = coroutine.create(function() local x = {7} coroutine.yield() return x end)
    coroutine.resume(co)
    held = co
    local done = coroutine.wrap(function() return {} end) done()
    collectgarbage()
    for i = 1, 40 do live[i][3] = {i} end
    for i = 1, 300 do local s = 'm' .. i end";
const CHECK: &str = "
    for i, v in ipairs(live or {}) do
        assert(v[1] == i and v[2] == tostring(i) and (v[3] == nil or v[3][1] == i))
    end
    assert(held == nil or coroutine.status(held) == 'suspended')
    assert(getmetatable(kept).tag == 'kept')
    return true";

/// A new interpreter in `stress` with the script and its check loaded, a userdata kept and one left as garbage.
fn collected(stress: GcStress, script: &str) -> (Lua, Value, Value) {
    let mut lua = Lua::new(
        Limits {
            fuel_per_call: AMPLE,
            fuel_per_tick: AMPLE,
            ..Limits::DEFAULT
        },
        Keys {
            hash: [193, 271],
            random: 0,
        },
    )
    .expect("state");
    lua.set_gc_stress(stress);
    let run = lua.load("script", script.as_bytes()).expect("compiles");
    let check = lua.load("check", CHECK.as_bytes()).expect("compiles");
    let vm = lua.vm();
    let metatable = vm.new_table().expect("metatable");
    let (key, tag) = (
        vm.new_string(b"tag").expect("key"),
        vm.new_string(b"kept").expect("tag"),
    );
    vm.raw_set(metatable, key, tag).expect("field");
    let kept = vm.userdata(1u8, Some(metatable), 0).expect("userdata");
    vm.userdata(2u8, None, 0).expect("garbage userdata");
    lua.set_global("kept", kept).expect("global");
    for function in [run, check] {
        lua.pin(function).expect("root fits");
    }
    (lua, run, check)
}

/// The script's whole cost in `stress`.
fn cost(stress: GcStress, script: &str) -> u64 {
    let (mut lua, run, _) = collected(stress, script);
    lua.begin_tick();
    lua.call(run, &[]).expect("runs with ample fuel");
    AMPLE - lua.tick_fuel_left()
}

/// Stops the script by fuel at every `stride`-th fuel of its cost, then calls the check, collects whole and checks
/// again: whatever the collection was doing when the fuel ran out, the next call resumes it.
fn resumes_after_every_fuel_stop(stress: GcStress, script: &str, stride: usize) {
    let full = cost(stress, script);
    for fuel in (1..=full).step_by(stride) {
        let (mut lua, run, check) = collected(stress, script);
        lua.vm().limits.fuel_per_call = fuel;
        lua.begin_tick();
        let stopped = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| lua.call(run, &[])));
        let stopped = stopped
            .unwrap_or_else(|_| panic!("{stress:?}, fuel {fuel} of {full}: the script panicked"));
        assert!(
            stopped.is_ok() || matches!(stopped, Err(Error::Limit(Limit::FuelPerCall))),
            "{stress:?}, fuel {fuel} of {full}: {stopped:?}"
        );
        lua.vm().limits.fuel_per_call = AMPLE;
        for round in 0..2 {
            lua.begin_tick();
            let resumed =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| lua.call(check, &[])));
            let resumed = resumed.unwrap_or_else(|_| {
                panic!("{stress:?}, fuel {fuel} of {full}: the next call panicked")
            });
            assert_eq!(
                resumed,
                Ok(vec![Value::Boolean(true)]),
                "{stress:?}, fuel {fuel} of {full}, round {round}"
            );
            lua.collect()
                .unwrap_or_else(|error| panic!("{stress:?}, fuel {fuel} of {full}: {error}"));
        }
    }
}

#[test]
fn a_collection_stopped_by_fuel_anywhere_resumes_on_the_next_call() {
    resumes_after_every_fuel_stop(GcStress::Off, COLLECTED, 1);
}

#[test]
fn an_incremental_collection_stopped_by_fuel_anywhere_resumes_on_the_next_call() {
    resumes_after_every_fuel_stop(GcStress::Incremental, COLLECTED, 1);
}

/// A full collection at every instruction makes every fuel a collection's: a smaller script, every fuel of it.
#[test]
fn a_full_collection_stopped_by_fuel_anywhere_resumes_on_the_next_call() {
    let script = "live = {} for i = 1, 3 do live[i] = {i, tostring(i)} end local s = 'x' .. 1
        local co = coroutine.create(function() coroutine.yield() end) coroutine.resume(co) held = co";
    resumes_after_every_fuel_stop(GcStress::Full, script, 1);
}
