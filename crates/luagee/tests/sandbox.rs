//! The sandbox's bounds across calls, as a host sees them: the tick's fuel, all the source loaded, the collector
//! giving memory back, and runs that repeat exactly.

use std::cell::RefCell;
use std::rc::Rc;

use luagee::{Error, GcStress, Limit, Limits, Lua, Value};

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
    let mut lua = lua(Limits::DEFAULT
        .with_fuel_per_call(1_000_000)
        .with_fuel_per_tick(1_000_000));
    let spin = lua
        .load("spin", b"for i = 1, 600000 do end")
        .expect("compiles");
    lua.pin(spin).expect("root fits");
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
    let mut lua = lua(Limits::DEFAULT.with_scripts_size(1 << 16));
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
    let mut lua = lua(Limits::DEFAULT.with_memory(4 << 20));
    let churn = lua
        .load(
            "churn",
            b"local t = {} for i = 1, 20000 do t[i % 100 + 1] = {i, tostring(i)} end",
        )
        .expect("compiles");
    lua.pin(churn).expect("root fits");
    for _ in 0..50 {
        lua.begin_tick();
        assert!(
            lua.call(churn, &[]).is_ok(),
            "the garbage of every call is reclaimed"
        );
    }
    assert!(lua.memory() < 4 << 20);
}

/// What only an upvalue cell or a userdata's metatable holds is alive: a collection, explicit or automatic, in every
/// collector mode, leaves the closure and the userdata able to use it.
#[test]
fn objects_held_only_by_upvalues_and_userdata_metatables_survive_collection() {
    for stress in [GcStress::Off, GcStress::Incremental, GcStress::Full] {
        let mut lua = lua(Limits::DEFAULT);
        lua.set_gc_stress(stress);
        let reader = lua
            .exec(
                "upvalues",
                b"local t = {1, 2, x = 'held'} local s = string.rep('s', 40) local function inner() return #s end
                  return function(u) collectgarbage() return #t, t.x, inner(), getmetatable(u).tag end",
            )
            .expect("prepares")[0];
        lua.pin(reader).expect("root fits");
        let vm = lua.vm();
        let metatable = vm.new_table().expect("metatable");
        let tag = vm.new_string(b"meta").expect("tag");
        let key = vm.new_string(b"tag").expect("key");
        vm.raw_set(metatable, key, tag).expect("field");
        let object = vm.userdata(7u8, Some(metatable), 0).expect("userdata");
        lua.pin(object).expect("root fits");
        lua.collect().expect("collects");
        lua.collect().expect("collects again");
        lua.begin_tick();
        let values = lua
            .call(reader, &[object])
            .expect("every held object is alive");
        let texts: Vec<_> = [values[1], values[3]]
            .map(|value| lua.bytes(value).map(<[u8]>::to_vec))
            .into();
        assert_eq!(values[0], Value::Number(2.0), "{stress:?}: the table");
        assert_eq!(
            values[2],
            Value::Number(40.0),
            "{stress:?}: the string and function"
        );
        assert_eq!(
            texts,
            [Some(b"held".to_vec()), Some(b"meta".to_vec())],
            "{stress:?}: the table's field and the metatable"
        );
    }
}

/// Large tables and coroutine stacks are traced a piece at a time while the script runs between the pieces: values
/// moved by a rebuild of the hash part, by a migration into the array part, by sorting, or by a coroutine running, are
/// all still found.
#[test]
fn objects_moved_while_a_large_object_is_half_traced_survive_collection() {
    // A full collection at every instruction never leaves an object half traced: the other two modes do.
    for stress in [GcStress::Off, GcStress::Incremental] {
        let mut lua = lua(Limits::DEFAULT
            .with_fuel_per_call(50_000_000)
            .with_fuel_per_tick(50_000_000));
        lua.set_gc_stress(stress);
        lua.exec(
            "moves",
            br#"
            local t = {}
            for i = 2, 6000 do t[i] = {i} end
            local co = coroutine.wrap(function(...)
                local held = {...}
                while true do coroutine.yield(#held, held[#held][1]) end
            end)
            for round = 1, 4 do
                for i = 1, 300 do t[-i] = {-i} end
                if round == 2 then t[1] = {1} end
                for i = 1, 300 do t[-i] = nil end
                local a, b = co({round}, {round + 1})
                assert(a == 2 and b == 2)
            end
            table.sort(t, function(x, y) return x[1] > y[1] end)
            for _ = 1, 3 do collectgarbage("step") end
            for i = 1, 6000 do assert(t[i][1] == 6001 - i, i) end
        "#,
        )
        .unwrap_or_else(|error| panic!("{stress:?}: {error}"));
    }
}

/// A collection of a large heap costs fuel like any work: it runs only as far as the call's fuel goes, and the calls
/// after it finish it.
#[test]
fn a_collection_runs_only_as_far_as_the_fuel_and_later_calls_finish_it() {
    let mut lua = lua(Limits::DEFAULT
        .with_fuel_per_call(40_000)
        .with_fuel_per_tick(40_000)
        .with_memory(128 << 20));
    // Filling pays for the collections it sets off too, so some calls run out of fuel on the way; what they made
    // stays.
    let fill = lua
        .load(
            "fill",
            b"kept = kept or {} for i = 1, 500 do kept[#kept + 1] = ('x'):rep(200) .. #kept end",
        )
        .expect("compiles");
    lua.pin(fill).expect("root fits");
    let count = lua.load("count", b"return #kept").expect("compiles");
    lua.pin(count).expect("root fits");
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
    lua.pin(idle).expect("root fits");
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
    let mut lua = lua(Limits::DEFAULT.with_memory(4 << 20));
    let hoard = lua
        .load(
            "hoard",
            b"kept = kept or {} for i = 1, 5000 do kept[#kept + 1] = ('x'):rep(64) .. #kept end",
        )
        .expect("compiles");
    lua.pin(hoard).expect("root fits");
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
    let mut lua = lua(Limits::DEFAULT.with_fuel_per_call(100_000));
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

#[test]
fn table_move_refuses_unaffordable_work_before_mutating_the_destination() {
    let mut lua = lua(Limits::DEFAULT
        .with_fuel_per_call(10_000)
        .with_fuel_per_tick(10_000));
    let ended = lua.exec(
        "move",
        b"destination = {99}; table.move({1}, 1, 100000, 1, destination)",
    );
    assert!(
        matches!(ended, Err(Error::Limit(Limit::FuelPerCall))),
        "{ended:?}"
    );
    lua.begin_tick();
    assert_eq!(
        lua.exec("read", b"return destination[1]")
            .expect("still usable"),
        vec![Value::Number(99.0)]
    );
}

#[test]
fn table_move_nonprogressing_ranges_exhaust_fuel_without_hanging() {
    for range in [
        "1e300,1e300,0",
        "math.huge,math.huge,0",
        "-math.huge,0,0",
        "2^53-1,2^53,0",
    ] {
        let mut lua = lua(Limits::DEFAULT);
        let source = format!("table.move({{}}, {range})");
        let ended = lua.exec("move", source.as_bytes());
        assert!(
            matches!(ended, Err(Error::Limit(Limit::FuelPerCall))),
            "{range}: {ended:?}"
        );
    }
}

#[test]
fn table_move_destination_growth_is_accounted_under_the_memory_limit() {
    let mut lua = lua(Limits::DEFAULT
        .with_memory(256 << 10)
        .with_fuel_per_call(1_000_000)
        .with_fuel_per_tick(1_000_000));
    lua.exec(
        "setup",
        b"source = {}; for i=1,1024 do source[i]=i end; destination={}",
    )
    .expect("source fits");
    let ended = lua.exec(
        "move",
        b"for i=1,8 do table.move(source,1,1024,-100000*i,destination) end",
    );
    assert!(
        matches!(ended, Err(Error::Limit(Limit::Memory))),
        "{ended:?}"
    );
}

#[test]
fn format_pointer_identity_is_reproducible_and_survives_collection() {
    let source = b"local t={}; local id=string.format('%p',t); collectgarbage(); assert(id==string.format('%p',t)); local released=string.format('%p',{}); collectgarbage(); assert(released~=string.format('%p',{})); print(id, string.format('%p',print), string.format('%p','text'))";
    let run = || {
        let output = Rc::new(RefCell::new(Vec::new()));
        let sink = output.clone();
        let mut lua = lua(Limits::DEFAULT);
        lua.set_print(Box::new(move |bytes| {
            sink.borrow_mut().extend_from_slice(bytes)
        }));
        lua.exec("identity", source)
            .expect("identity relations hold");
        output.take()
    };
    assert_eq!(
        run(),
        run(),
        "identities depend only on local handles, including generations and object kinds"
    );
}

#[test]
fn format_pointer_distinguishes_userdata_without_stringifying_it() {
    let mut lua = lua(Limits::DEFAULT);
    let first = lua.vm().userdata(1u8, None, 0).expect("first userdata");
    lua.pin(first).expect("root");
    let second = lua.vm().userdata(2u8, None, 0).expect("second userdata");
    lua.pin(second).expect("root");
    let check = lua.load("userdata_identity", b"local a,b=...; assert(string.format('%p',a)==string.format('%p',a)); assert(string.format('%p',a)~=string.format('%p',b)); for _,v in ipairs({a,b,print}) do assert(tostring(v):match('0x%x+$')==string.format('%p',v)) end").expect("compiles");
    lua.call(check, &[first, second])
        .expect("distinct stable identities");
}

#[test]
fn format_quote_charges_for_escape_expansion_before_copying() {
    let mut lua = lua(Limits::DEFAULT
        .with_fuel_per_call(5_000)
        .with_fuel_per_tick(5_000)
        .with_string_length(1 << 20));
    lua.exec(
        "setup",
        b"source=string.rep(string.char(1),12000); template=string.rep('%q',8)",
    )
    .expect("inputs fit");
    lua.begin_tick();
    let ended = lua.exec(
        "quote",
        b"return string.format(template,source,source,source,source,source,source,source,source)",
    );
    assert!(
        matches!(ended, Err(Error::Limit(Limit::FuelPerCall))),
        "{ended:?}"
    );
}

#[test]
fn format_quote_checks_expanded_string_and_heap_bounds() {
    let mut lua = lua(Limits::DEFAULT.with_string_length(100));
    let ended = lua.exec(
        "quote",
        b"return string.format('%q', string.rep(string.char(1),50))",
    );
    assert!(
        matches!(ended, Err(Error::Runtime { ref message, .. }) if message.contains("string length overflow")),
        "{ended:?}"
    );
    let mut lua = self::lua(
        Limits::DEFAULT
            .with_memory(256 << 10)
            .with_fuel_per_call(1_000_000)
            .with_fuel_per_tick(1_000_000),
    );
    lua.exec("setup", b"source=string.rep(string.char(1),40000)")
        .expect("source fits");
    let ended = lua.exec(
        "quote",
        b"return string.format('%q%q%q',source,source,source)",
    );
    assert!(
        matches!(ended, Err(Error::Limit(Limit::Memory))),
        "{ended:?}"
    );
}

#[test]
fn format_repeated_flags_spend_fuel_in_proportion_to_template_length() {
    let mut lua = lua(Limits::DEFAULT
        .with_fuel_per_call(5_000)
        .with_fuel_per_tick(5_000));
    lua.exec("setup", b"template='%'..string.rep('-',8000)..'d'")
        .expect("template fits");
    lua.begin_tick();
    let ended = lua.exec("format", b"for i=1,10 do string.format(template,1) end");
    assert!(
        matches!(ended, Err(Error::Limit(Limit::FuelPerCall))),
        "{ended:?}"
    );
}

#[test]
fn numeric_for_signed_zero_steps_remain_cancellable_by_fuel() {
    for (name, source) in [
        ("positive", "for i=1,2,0.0 do end"),
        ("negative", "for i=2,1,-0.0 do end"),
    ] {
        let mut lua = lua(Limits::DEFAULT);
        let ended = lua.exec(name, source.as_bytes());
        assert!(
            matches!(ended, Err(Error::Limit(Limit::FuelPerCall))),
            "{name}: {ended:?}"
        );
    }
}

#[test]
fn native_error_names_charge_fuel_for_the_bytes_they_render() {
    let mut lua = lua(Limits::DEFAULT
        .with_fuel_per_call(10_000)
        .with_fuel_per_tick(10_000));
    let name = "n".repeat(4096);
    let source = format!(
        "local t={{['{name}']=math.abs}}; for i=1,50 do pcall(function() t['{name}'](false) end) end"
    );
    let function = lua
        .load("named_errors", source.as_bytes())
        .expect("source fits");
    let ended = lua.call(function, &[]);
    assert!(
        matches!(ended, Err(Error::Limit(Limit::FuelPerCall))),
        "{ended:?}"
    );
}
