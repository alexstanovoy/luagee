//! The collector: pieces of a large object with the script mutating the object between two pieces, and collections
//! stopped by fuel anywhere and resumed by the next call.

use super::Phase;
use crate::heap::Function;
use crate::heap::Partial;
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
        if let Some(Partial::Table { entries, .. }) = vm.heap.partial
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
    while let Some(Partial::Thread { .. }) = vm.heap.partial {
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

/// An interpreter that ran `prepare` (with a 100000-item array `t` of `{i}`, traced from its end so that its first
/// items stay untraced for some hundreds of instructions), collected, then started marking.
fn marking(prepare: &str) -> Lua {
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
    let source = format!("t = {{}} for i = 1, 100000 do t[i] = {{i}} end {prepare}");
    lua.exec("prepare", source.as_bytes()).expect("prepares");
    lua.collect().expect("collects");
    lua.vm().gc_start().expect("marking starts");
    lua
}

/// Runs `moves` while marking, finishes the collection and runs `check`.
fn moved_then_checked(mut lua: Lua, moves: &str, check: &str) {
    assert_eq!(lua.vm().heap.phase, Phase::Marking);
    lua.exec("moves", moves.as_bytes()).expect("moves");
    lua.collect().expect("collects");
    lua.exec("check", check.as_bytes())
        .expect("what was moved is alive");
}

/// A value stored into an upvalue cell already traced is grayed by the store.
#[test]
fn a_store_into_a_traced_cell_keeps_the_value_stored() {
    let mut lua = marking("local c f = function(v) if v then c = v end return c end");
    let f = lua.exec("f", b"return f").expect("f")[0];
    let vm = lua.vm();
    let Value::Function(id) = f else {
        panic!("a function")
    };
    let Function::Lua { upvalues, .. } = vm.heap.functions.get(id.0) else {
        panic!("a Lua function")
    };
    let cell = Value::Cell(upvalues[0]);
    vm.trace(f);
    vm.trace(cell);
    moved_then_checked(lua, "f(t[1]) t[1] = nil", "assert(f()[1] == 1)");
}

/// A closure made while marking is born marked: the cells it captures, made before marking and held by nothing
/// traced, are grayed when it is made.
#[test]
fn a_closure_made_while_marking_keeps_the_cells_it_captures() {
    let lua = marking("local o = {1} t[1] = function() return function() return o end end");
    moved_then_checked(lua, "g = t[1]() t[1] = nil", "assert(g()[1] == 1)");
}

/// A coroutine made while marking is born marked: its function, held by nothing traced, is grayed when it is made.
#[test]
fn a_coroutine_made_while_marking_keeps_its_function() {
    let lua = marking("local o = {1} t[1] = function() return o end");
    moved_then_checked(
        lua,
        "co = coroutine.create(t[1]) t[1] = nil",
        "local ok, o = coroutine.resume(co) assert(ok and o[1] == 1)",
    );
}

/// The handler of an `xpcall` a suspended coroutine is inside, held by nothing else, survives.
#[test]
fn a_suspended_coroutine_keeps_the_handler_of_its_xpcall() {
    let mut lua = marking(
        "co = coroutine.create(function()
             local o = {1}
             return xpcall(function() coroutine.yield() error('e') end, function() return o end)
         end)
         coroutine.resume(co)",
    );
    lua.collect().expect("collects");
    lua.exec(
        "check",
        b"local ok, handled, o = coroutine.resume(co) assert(ok and not handled and o[1] == 1)",
    )
    .expect("the handler is alive");
}

/// What a half-traced table has left is clamped to what it now holds: a rebuild that drops removed entries below
/// the cursor, or an array part shorter than when its trace began, ends the pieces instead of reading past them.
/// The cursors are set past the table's parts here (its values are scalars, so skipping a part loses nothing).
#[test]
fn a_half_traced_table_that_shrank_is_traced_to_its_end() {
    for (entries, array) in [(1000, None), (0, Some(1000))] {
        let mut lua = marking("u = {1, 2, [10] = 3}");
        let u = lua.exec("u", b"return u").expect("u")[0];
        let Value::Table(id) = u else {
            panic!("a table")
        };
        let vm = lua.vm();
        while vm.heap.partial.is_some() {
            vm.trace_gray(16).expect("ends the object half traced");
        }
        // Traced already or not: tracing it again in pieces is what is tested.
        vm.heap.tables.mark(id.0);
        vm.heap.partial = Some(Partial::Table { id, entries, array });
        vm.trace_gray(AMPLE as usize).expect("traces");
        assert!(vm.heap.partial.is_none(), "the pieces ended");
        lua.collect().expect("collects");
        lua.exec("check", b"assert(u[1] == 1 and u[10] == 3)")
            .expect("checks");
    }
}

/// A key whose entry was removed is not kept alive by the table it was removed from.
#[test]
fn a_removed_key_does_not_keep_its_object_alive() {
    let mut lua = marking("");
    lua.collect().expect("collects");
    let before = lua.vm().heap.tables.live();
    lua.exec("remove", b"local k = {} h = {} h[k] = 1 h[k] = nil")
        .expect("removes");
    lua.collect().expect("collects");
    assert_eq!(
        lua.vm().heap.tables.live(),
        before + 1,
        "only `h` is left of the two tables"
    );
}

/// An array part shifted up while half traced (a prepend that moves values without writing them) keeps the moved
/// values: `Heap::array_shifted` grows the untraced part by the shift. Without it the values moved past the cursor
/// would be swept while still in the table.
#[test]
fn values_shifted_past_a_half_traced_array_cursor_are_still_traced() {
    let mut lua = marking("u = {} for i = 1, 20000 do u[i] = {i} end");
    let Value::Table(u) = lua.exec("u", b"return u").expect("u")[0] else {
        panic!("a table")
    };
    let vm = lua.vm();
    let mut gap = None;
    for _ in 0..1_000_000 {
        if let Some(Partial::Table {
            id,
            array: Some(from),
            ..
        }) = vm.heap.partial
            && id == u
            && from < 15_000
        {
            gap = Some(20_000 - from);
            break;
        }
        vm.trace_gray(16).expect("traces");
    }
    let gap = gap.expect("the array part is half traced");
    vm.heap.tables.get_mut(u.0).shift_array(gap);
    vm.heap.array_shifted(u, gap);
    lua.collect().expect("collects");
    let check = format!("for i = 1, 20000 do assert(u[i + {gap}][1] == i, i) end");
    lua.exec("check", check.as_bytes())
        .expect("every shifted value is alive");
}

/// Coroutines resuming one another hold each other through their callers' stacks: a chain of them, made inline and
/// held by nothing else, survives collections at its innermost level in every collector mode.
#[test]
fn a_chain_of_resumed_coroutines_survives_collections_at_its_innermost_level() {
    for stress in [GcStress::Off, GcStress::Incremental, GcStress::Full] {
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
        lua.exec(
            "chain",
            b"local function level(d)
                  local o = {d}
                  if d == 0 then collectgarbage() collectgarbage() return o[1] end
                  local inner = coroutine.wrap(level)(d - 1)
                  collectgarbage()
                  return o[1] + inner
              end
              assert(coroutine.wrap(level)(20) == 210)
              local ok, sum = coroutine.resume(coroutine.create(level), 20)
              assert(ok and sum == 210)",
        )
        .unwrap_or_else(|error| panic!("{stress:?}: {error}"));
    }
}
