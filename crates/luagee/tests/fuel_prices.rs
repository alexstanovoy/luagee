//! The prices of the operations whose fuel is paid before the work (README, "Fuel paid before the work"), pinned: a
//! chunk run whole succeeds with exactly its price, one fuel less is refused, and the price is the one recorded.

use luagee::{Error, Keys, Limit, Limits, Lua};

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
        Limits {
            fuel_per_call: fuel,
            fuel_per_tick: fuel,
            ..Limits::DEFAULT
        },
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
    ("local k=0 load(function() k=k+1 if k==1 then return string.rep(' ',4000) end end)", 1531),
    ("local k=0 load(function() k=k+1 if k<=200 then return '--x\\n' .. ' ' end end)", 2867),
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
