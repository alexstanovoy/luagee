//! The prices of the operations whose fuel is paid before the work (README, "Fuel paid before the work"), pinned: a
//! chunk run whole succeeds with exactly its price, one fuel less is refused, and the price is the one recorded.

use std::rc::Rc;

use luagee::{Error, Keys, Limit, Limits, Lua, Value};

/// The fuel a chunk costs, found by running it with ample fuel.
fn price(source: &str) -> u64 {
    let budget = 1 << 40;
    let mut lua = interpreter(budget);
    lua.exec("priced", source.as_bytes())
        .unwrap_or_else(|error| panic!("{source}: {error}"));
    budget - lua.tick_fuel_left()
}

fn interpreter(fuel: u64) -> Lua {
    Lua::new(
        Limits::DEFAULT
            .with_fuel_per_call(fuel)
            .with_fuel_per_tick(fuel),
        Keys {
            hash: [193, 271],
            random: 0,
        },
    )
    .expect("state")
}

#[rustfmt::skip]
const PRICES: &[(&str, u64)] = &[
    ("local t={} for i=1,1000 do t[i+0.5]=i end return table.maxn(t)", 5444),
    ("local t={} for i=1,1000 do t[i]=i end for i=1,1000 do t[i]=nil end return table.maxn(t)", 7279),
    ("local t={} for i=1,1000 do t[-i]=i end for i=1,1000 do t[-i]=nil end return table.maxn(t)", 9448),
    ("local t={} for i=1,1000 do t[-i]=i end table.foreach(t,function() end)", 7326),
    ("local t={} for i=1,1000 do t[-i]=i end table.foreach(t,coroutine.running)", 7323),
    ("local t={} for i=1,1000 do t[i]=i end table.foreachi(t,coroutine.running)", 5153),
    ("local t={} for i=1,1000 do t[i]=i end table.sort(t,rawequal)", 30394),
    ("string.gsub(string.rep('x',1000),'.',type)", 12895),
    ("pcall(function() assert(false,string.rep('x',1000)) end)", 269),
    ("pcall(function() local t return t.x end)", 16),
    ("string.match('y','['..string.rep('x',1000)..']')", 520),
    ("pcall(string.match,'y','['..string.rep('x',1000))", 333),
    ("string.find('y','abcdefghijklmnopq')", 9),
    ("for i=1,1100 do local s='k'..i end collectgarbage()", 10259),
    ("local function f(d) local a,b,c,e,g if d>0 then return (f(d-1)) end collectgarbage() end f(64)", 844),
    ("local t={} for i=1,1000 do t[i]={} end collectgarbage()", 10299),
    ("local k=0 load(function() k=k+1 if k==1 then return string.rep(' ',4000) end end)", 1539),
    ("load(string.rep('a=a ', 1000))", 25521),
    ("load(string.rep('--x\\n', 1000))", 1521),
    ("load(string.rep('x={1,2,3}\\n', 1000))", 75771),
    ("local k=0 load(function() k=k+1 if k<=200 then return '--x\\n' .. ' ' end end)", 2875),
];

#[test]
fn operations_paid_before_their_work_cost_their_recorded_prices() {
    let mut wrong = Vec::new();
    for &(source, recorded) in PRICES {
        let price = price(source);
        if price != recorded {
            wrong.push(format!("{source}: {price}, recorded {recorded}"));
            continue;
        }
        let exact = interpreter(price).exec("exact", source.as_bytes());
        assert!(
            exact.is_ok(),
            "{source}: the exact price suffices: {exact:?}"
        );
        let short = interpreter(price - 1).exec("short", source.as_bytes());
        assert!(
            matches!(short, Err(Error::Limit(Limit::FuelPerCall))),
            "{source}: one fuel less is refused: {short:?}"
        );
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// A host function that does nothing, and a Lua function that does nothing, as `Value`s of `lua`.
fn empty_functions(lua: &mut Lua) -> (Value, Value) {
    let native = lua
        .function("nothing", Rc::new(|vm, _args| vm.results(&[])))
        .expect("a host function");
    let script = lua
        .exec("empty", b"return function() end")
        .expect("a function")[0];
    (native, script)
}

fn arguments(count: usize) -> Vec<Value> {
    vec![Value::Number(1.0); count]
}

/// The fuel a top-level `Lua::call` of `function` with `count` arguments spends.
fn host_call_price(lua: &mut Lua, function: Value, count: usize) -> u64 {
    lua.begin_tick();
    let before = lua.tick_fuel_left();
    lua.call(function, &arguments(count)).expect("the call");
    before - lua.tick_fuel_left()
}

/// The fuel a `Vm::call` of `function` with `count` arguments spends inside a host function (the outer call's own
/// price taken off).
fn callback_call_price(lua: &mut Lua, function: Value, count: usize) -> u64 {
    let caller = lua
        .function(
            "caller",
            Rc::new(move |vm, _args| {
                vm.call(function, &arguments(count))?;
                vm.results(&[])
            }),
        )
        .expect("a host function");
    let (idle, _) = empty_functions(lua);
    host_call_price(lua, caller, 0) - host_call_price(lua, idle, 0)
}

#[test]
fn a_host_call_pays_for_its_arguments_and_one_more_for_the_call_of_a_native_or_the_return_of_a_lua_function()
 {
    let mut lua = interpreter(1 << 20);
    let (native, script) = empty_functions(&mut lua);
    // `Lua::call`: `1 + arguments / 8` to cross the boundary, then the callee. A Lua callee runs instructions (an
    // empty one, its return: 1); a native has none, so the call itself costs 1, as a native called by a native does.
    // So a call is never free and an empty native costs what an empty Lua function does.
    for (count, crossing) in [(0, 1), (7, 1), (8, 2), (16, 3)] {
        assert_eq!(
            host_call_price(&mut lua, native, count),
            crossing + 1,
            "a native, {count} arguments"
        );
        assert_eq!(
            host_call_price(&mut lua, script, count),
            crossing + 1,
            "an empty Lua function, {count} arguments"
        );
    }
}

#[test]
fn a_call_from_a_host_callback_pays_both_boundary_crossings_and_one_for_the_call_of_a_native_or_a_return()
 {
    let mut lua = interpreter(1 << 20);
    let (native, script) = empty_functions(&mut lua);
    // `Vm::call`: `1 + arguments / 8` in, `1 + results / 8` out, and the callee as above.
    for (count, crossing) in [(0, 1), (8, 2), (16, 3)] {
        assert_eq!(
            callback_call_price(&mut lua, native, count),
            crossing + 1 + 1,
            "a native, {count} arguments"
        );
        assert_eq!(
            callback_call_price(&mut lua, script, count),
            crossing + 1 + 1,
            "an empty Lua function, {count} arguments"
        );
    }
}

/// A block of ordinary code, about 280 bytes: the README quotes what loading it repeated costs.
const ORDINARY: &str = "do
  local function clamp(value, low, high)
    if value < low then return low end
    if value > high then return high end
    return value
  end
  local items = { 'alpha', 'beta', 'gamma' }
  local counts = {}
  for index, name in ipairs(items) do
    counts[name] = clamp(index * 3, 1, 5) -- keep it small
  end
end
";

/// What `load` of `ORDINARY` repeated costs by itself: the price of the chunk that builds the source and loads it,
/// less the price of the chunk that only builds it. Besides the bytes it loads.
fn ordinary_load_price(repeats: usize) -> (usize, u64) {
    let building =
        format!("local block = [==[{ORDINARY}]==] local s = string.rep(block, {repeats})");
    let loading = format!("{building} assert(load(s))");
    (ORDINARY.len() * repeats, price(&loading) - price(&building))
}

#[test]
fn loading_ordinary_code_costs_the_prices_the_readme_gives() {
    // (repeats, bytes, fuel). Before tokens were paid for, `load` cost a quarter fuel a byte: 319 and 28,710.
    let pinned = [(4, 1276, 2636), (360, 114_840, 236_083)];
    let mut wrong = Vec::new();
    for (repeats, bytes, recorded) in pinned {
        let (loaded, fuel) = ordinary_load_price(repeats);
        eprintln!(
            "{repeats}: {loaded} bytes, before {}, after {fuel}",
            loaded / 4
        );
        if (loaded, fuel) != (bytes, recorded) {
            wrong.push(format!(
                "{repeats} repeats: {loaded} bytes, {fuel} fuel; recorded {bytes}, {recorded}"
            ));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
