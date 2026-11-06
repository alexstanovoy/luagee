//! The sandbox's bounds across calls, as a host sees them: the tick's fuel, all the source loaded, the collector
//! giving memory back, and runs that repeat exactly.

use std::cell::RefCell;
use std::rc::Rc;

use luagee::{Error, Limit, Limits, Lua, Value};

fn lua(limits: Limits) -> Lua {
    Lua::new(
        limits,
        luagee::Keys {
            hash: [3, 3],
            random: 3,
        },
    )
    .expect("an interpreter")
}

#[test]
fn calls_of_one_tick_share_its_fuel_and_a_new_tick_refills_it() {
    let mut lua = lua(Limits {
        fuel_per_call: 1_000_000,
        fuel_per_tick: 1_000_000,
        ..Limits::DEFAULT
    });
    let spin = lua
        .load("spin", b"for i = 1, 600000 do end")
        .expect("compiles");
    lua.pin(spin);
    assert!(lua.call(spin, &[]).is_ok(), "the first call fits the tick");
    let second = lua.call(spin, &[]);
    assert!(
        matches!(second, Err(Error::Limit(Limit::FuelPerTick))),
        "the second call runs past the tick's fuel: {second:?}"
    );
    lua.begin_tick();
    assert!(lua.call(spin, &[]).is_ok(), "a new tick has fuel again");
}

#[test]
fn all_the_source_one_interpreter_loads_is_bounded() {
    let mut lua = lua(Limits {
        scripts_size: 1 << 16,
        ..Limits::DEFAULT
    });
    let ended = lua.exec(
        "load",
        b"local s = 'return 1' .. (' '):rep(8000) for i = 1, 100 do loadstring(s) end",
    );
    assert!(
        matches!(ended, Err(Error::Limit(Limit::ScriptsSize))),
        "{ended:?}"
    );
}

#[test]
fn garbage_made_across_many_calls_is_collected_under_a_small_cap() {
    let mut lua = lua(Limits {
        memory: 4 << 20,
        ..Limits::DEFAULT
    });
    let churn = lua
        .load(
            "churn",
            b"local t = {} for i = 1, 20000 do t[i % 100 + 1] = {i, tostring(i)} end",
        )
        .expect("compiles");
    lua.pin(churn);
    for _ in 0..50 {
        lua.begin_tick();
        assert!(
            lua.call(churn, &[]).is_ok(),
            "the garbage of every call is reclaimed"
        );
    }
    assert!(lua.memory() < 4 << 20);
}

/// A collection of a large heap costs fuel like any work: it runs only as far as the call's fuel goes, and the calls
/// after it finish it.
#[test]
fn a_collection_runs_only_as_far_as_the_fuel_and_later_calls_finish_it() {
    let mut lua = lua(Limits {
        fuel_per_call: 40_000,
        fuel_per_tick: 40_000,
        memory: 128 << 20,
        ..Limits::DEFAULT
    });
    // Filling pays for the collections it sets off too, so some calls run out of fuel on the way; what they made
    // stays.
    let fill = lua
        .load(
            "fill",
            b"kept = kept or {} for i = 1, 500 do kept[#kept + 1] = ('x'):rep(200) .. #kept end",
        )
        .expect("compiles");
    lua.pin(fill);
    let count = lua.load("count", b"return #kept").expect("compiles");
    lua.pin(count);
    for _ in 0..4000 {
        lua.begin_tick();
        let _ = lua.call(fill, &[]);
        lua.begin_tick();
        if matches!(lua.call(count, &[]).as_deref(), Ok([Value::Number(n)]) if *n >= 200_000.0) {
            break;
        }
    }
    lua.collect().expect("the tables fit the cap");
    let full = lua.memory();
    lua.begin_tick();
    let dropped = lua.exec("drop", b"kept = nil collectgarbage()");
    assert!(
        matches!(dropped, Err(Error::Limit(Limit::FuelPerCall))),
        "a whole collection of 200000 strings needs more than one call's fuel: {dropped:?}"
    );
    assert!(
        lua.memory() >= full,
        "the sweep did not run past the fuel: {} < {full}",
        lua.memory()
    );
    let idle = lua
        .load("idle", b"local x = 0 for i = 1, 1000 do x = x + i end")
        .expect("compiles");
    lua.pin(idle);
    for _ in 0..40 {
        lua.begin_tick();
        let _ = lua.call(idle, &[]);
    }
    // What stays is the slots the strings were in, and the index of strings.
    assert!(
        lua.memory() < full / 4,
        "later calls finished the collection: {} of {full}",
        lua.memory()
    );
}

#[test]
fn memory_a_script_keeps_across_calls_runs_into_the_cap() {
    let mut lua = lua(Limits {
        memory: 4 << 20,
        ..Limits::DEFAULT
    });
    let hoard = lua
        .load(
            "hoard",
            b"kept = kept or {} for i = 1, 5000 do kept[#kept + 1] = ('x'):rep(64) .. #kept end",
        )
        .expect("compiles");
    lua.pin(hoard);
    let mut ended = Ok(Vec::new());
    for _ in 0..100 {
        lua.begin_tick();
        ended = lua.call(hoard, &[]);
        if ended.is_err() {
            break;
        }
    }
    assert!(
        matches!(ended, Err(Error::Limit(Limit::Memory))),
        "{ended:?}"
    );
}

#[test]
fn an_interpreter_runs_again_after_any_error() {
    let mut lua = lua(Limits {
        fuel_per_call: 100_000,
        ..Limits::DEFAULT
    });
    assert!(lua.exec("loop", b"while true do end").is_err());
    lua.begin_tick();
    assert!(lua.exec("error", b"error('x')").is_err());
    lua.begin_tick();
    let after = lua.exec("after", b"return 1 + 1").expect("runs");
    assert_eq!(after, vec![Value::Number(2.0)]);
}

#[test]
fn the_same_seed_and_calls_print_the_same() {
    let program = br#"
        local t = {}
        for i = 1, 200 do t[tostring(i * 7919 % 1000)] = i end
        local order = {}
        for k in pairs(t) do order[#order + 1] = k end
        print(table.concat(order, ","), math.random(1, 1000000), tostring({}), tostring(print))
    "#;
    let run = || {
        let printed = Rc::new(RefCell::new(Vec::new()));
        let mut lua = lua(Limits::DEFAULT);
        let sink = printed.clone();
        lua.set_print(Box::new(move |line| {
            sink.borrow_mut().extend_from_slice(line)
        }));
        lua.exec("program", program).expect("runs");
        printed.take()
    };
    assert_eq!(run(), run());
}
