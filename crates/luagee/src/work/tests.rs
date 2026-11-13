//! Fuel is proportional to the work it pays for, and paid before the work. For every operation whose work a script
//! sizes, at three sizes: the counted work stays within the fuel spent; a call given half that fuel is refused having
//! done no more work than that half pays for, and leaves the state as it was; the exact fuel suffices and one less is
//! refused. Work is counted at the price the operation charges for it (see `Work::per_fuel`).

use super::Work;
use crate::{Error, Keys, Limit, Limits, Lua, Value};

/// Every kind of work, in the order of its discriminant.
const ALL: [Work; 4] = [Work::Slots, Work::Bytes, Work::Collector, Work::Steps];

/// The units of a kind of work one fuel pays for: the price the operations charge for it.
const fn per_fuel(work: Work) -> u64 {
    match work {
        Work::Slots => crate::vm::VALUES_PER_FUEL as u64,
        Work::Bytes => 16,
        Work::Collector => crate::vm::GC_OBJECTS_PER_FUEL as u64,
        Work::Steps => 1,
    }
}

std::thread_local! {
    static DONE: std::cell::Cell<[u64; ALL.len()]> = const { std::cell::Cell::new([0; ALL.len()]) };
}

pub(super) fn record(work: Work, units: usize) {
    DONE.with(|done| {
        let mut counts = done.get();
        counts[work as usize] += units as u64;
        done.set(counts);
    });
}

/// The work recorded since the last `take`, in fuel at each kind's price (rounded up), and in units.
fn take() -> (u64, u64) {
    let counts = DONE.with(|done| done.replace([0; ALL.len()]));
    let fuel = ALL
        .iter()
        .map(|&kind| counts[kind as usize].div_ceil(per_fuel(kind)))
        .sum();
    (fuel, counts.iter().sum())
}

/// Rounding: each kind of work may round up by one fuel.
const SLACK: u64 = 2;
const SIZES: [usize; 3] = [64, 1024, 16384];
const AMPLE: u64 = 1 << 40;

/// An operation whose work grows with `n`: its name, the Lua that prepares the state (with `n` bound), the body of the
/// function measured, and an expression that is true while the state is as the preparation left it.
type Row = (&'static str, &'static str, &'static str, &'static str);

const CLEARED_ARRAY: &str = "local t={} for i=1,n do t[i]=i end for i=1,n do t[i]=nil end";
const REMOVED_KEYS: &str = "local t={} for i=1,n do t[-i]=i end for i=1,n do t[-i]=nil end";
const ARRAY: &str = "local t={} for i=1,n do t[i]=i end";
const HASH: &str = "local t={} for i=1,n do t[-i]=i end";
const TEXT: &str = "local s=string.rep('x',n)";
/// A coroutine suspended `n / 16` Lua frames deep, each with 50 nil registers, collecting before each yield.
const DEEP_REGISTERS: &str = "local function f(d) local a1,a2,a3,a4,a5,a6,a7,a8,a9,a10,a11,a12,a13,a14,a15,a16,a17,a18,a19,a20
    local b1,b2,b3,b4,b5,b6,b7,b8,b9,b10,b11,b12,b13,b14,b15,b16,b17,b18,b19,b20,b21,b22,b23,b24,b25,b26,b27,b28,b29,b30
    if d > 0 then f(d - 1) else while true do collectgarbage() coroutine.yield() end end end
    local co = coroutine.create(function() f(math.floor(n / 16)) end) coroutine.resume(co)";

// One row a line: the table reads as the list of operations it covers.
#[rustfmt::skip]
const ROWS: &[Row] = &[
    ("table.maxn, cleared array", CLEARED_ARRAY, "return table.maxn(t)", "next(t) == nil"),
    ("table.maxn, removed keys", REMOVED_KEYS, "return table.maxn(t)", "next(t) == nil"),
    ("table.maxn, live keys", "local t={} for i=1,n do t[i+0.5]=i end", "return table.maxn(t)", "t[1.5] == 1"),
    ("next, cleared array", CLEARED_ARRAY, "return next(t)", "#t == 0"),
    ("next, removed keys", REMOVED_KEYS, "return next(t)", "next(t) == nil"),
    ("table.foreach, removed keys", REMOVED_KEYS, "return table.foreach(t,print)", "next(t) == nil"),
    ("table.foreach, native callback", HASH, "return table.foreach(t,coroutine.running)", "t[-n] == n"),
    ("table.foreachi, native callback", ARRAY, "return table.foreachi(t,coroutine.running)", "#t == n"),
    ("table.sort, native comparator", ARRAY, "table.sort(t,rawequal)", "#t == n"),
    ("string.gsub, native replacement", TEXT, "return string.gsub(s,'.',type)", "#s == n"),
    ("assert, long message, Lua caller", TEXT, "return pcall(function() assert(false, s) end)", "#s == n"),
    ("error, long message, located", TEXT, "return pcall(function() error(s) end)", "#s == n"),
    ("error, long message, rethrown", TEXT, "return pcall(function() local _, e = pcall(error, s, 0) error(e) end)", "#s == n"),
    ("string.match, long set", "local p='['..string.rep('x',n)..']'", "return string.match('y',p)", "#p == n+2"),
    ("string.find, long set at each position", "local p='['..string.rep('x',n)..']'", "return string.find(string.rep('y',64),p)", "#p == n+2"),
    ("string.match, long set repeated", "local p='['..string.rep('a',n)..'x]*$'", "return string.match(string.rep('x',64),p)", "#p == n+5"),
    ("string.match, long set lazily repeated", "local p='['..string.rep('a',n)..'x]-$'", "return string.match(string.rep('x',64),p)", "#p == n+5"),
    ("string.find, long plain-looking pattern", "local p=string.rep('x',n)..'.'", "return string.find('y',p)", "#p == n+1"),
    // A coroutine that runs out of fuel dies: there the check is only that the call ran.
    ("collectgarbage, deep nil registers", DEEP_REGISTERS, "return coroutine.resume(co)", "true"),
    ("collectgarbage, live tables", "local t={} for i=1,n do t[i]={} end", "local _ = t collectgarbage()", "#t == n"),
    ("collectgarbage, one large table", ARRAY, "local _ = t collectgarbage()", "#t == n"),
    ("collectgarbage, one large hash", HASH, "local _ = t collectgarbage()", "t[-n] == n"),
    ("tonumber, long numeric string", "local s=string.rep(' ',n)..'1'", "return tonumber(s)", "#s == n+1"),
    ("arithmetic, long numeric string", "local s=string.rep(' ',n)..'1'", "return s + 1", "#s == n+1"),
    ("error naming, long field name", "local k=string.rep('k',n) local f=loadstring('local t={} return t.'..k..'.x')", "return pcall(f)", "#k == n"),
    ("a write moving hash keys into the array", "local t={} for i=2,n+1 do t[i]=i end", "t[1]=1", "t[1] == nil and t[n+1] == n+1"),
];

fn interpreter() -> Lua {
    Lua::new(
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
    .expect("state")
}

/// What one run did: its outcome, the work counted (in fuel at each kind's price, and in units) and the fuel spent.
struct Run {
    outcome: Result<(), Error>,
    work: u64,
    units: u64,
    fuel: u64,
    /// Whether the state is still as the preparation left it.
    kept: bool,
}

/// Prepares the row's state in a new interpreter and runs the operation with `budget` fuel for the call, then checks
/// the state with ample fuel.
fn run(&(name, prepare, operation, unchanged): &Row, n: usize, budget: u64) -> Run {
    let mut lua = interpreter();
    let source = format!(
        "local n = {n} {prepare} return function() {operation} end, function() return {unchanged} end"
    );
    let functions = lua.exec(name, source.as_bytes()).expect("prepares");
    for &function in &functions {
        lua.pin(function).expect("root fits");
    }
    lua.collect().expect("collects");
    take();
    lua.vm().limits.fuel_per_call = budget;
    lua.begin_tick();
    let outcome = lua.call(functions[0], &[]).map(drop);
    let (work, units) = take();
    let fuel = AMPLE - lua.tick_fuel_left();
    lua.vm().limits.fuel_per_call = AMPLE;
    lua.begin_tick();
    let check = lua.call(functions[1], &[]).expect("the state check runs");
    let kept = check == [Value::Boolean(true)];
    Run {
        outcome,
        work,
        units,
        fuel,
        kept,
    }
}

fn refused(run: &Run) -> bool {
    matches!(run.outcome, Err(Error::Limit(Limit::FuelPerCall)))
}

/// What one row breaks at one size, if anything.
fn violations(row: &Row, n: usize) -> Vec<String> {
    let full = run(row, n, AMPLE);
    if let Err(error) = &full.outcome {
        return vec![format!("fails with ample fuel: {error}")];
    }
    let (fuel, units) = (full.fuel, full.units);
    let mut found = Vec::new();
    if units < n as u64 {
        found.push(format!("the counter misses the work: {units} units"));
    }
    if full.work > fuel + SLACK {
        found.push(format!("{} fuel of work for {fuel} fuel", full.work));
    }
    let half = run(row, n, fuel / 2);
    if !half.kept {
        found.push("a call refused at half the fuel changed the state".to_owned());
    }
    if !refused(&half) {
        found.push(format!("half the fuel is not refused: {:?}", half.outcome));
    }
    // Paid before it is done: refused at half the fuel, the work done was paid out of that half.
    if half.work > fuel / 2 + SLACK {
        found.push(format!(
            "{} fuel of work done before refusing at {} fuel",
            half.work,
            fuel / 2
        ));
    }
    if let Err(error) = run(row, n, fuel).outcome {
        found.push(format!("the exact fuel ({fuel}) does not suffice: {error}"));
    }
    if !refused(&run(row, n, fuel - 1)) {
        found.push(format!("one fuel less than {fuel} is not refused"));
    }
    found
}

#[test]
fn script_sized_work_is_paid_in_proportion_and_before_it_is_done() {
    let failures: Vec<String> = ROWS
        .iter()
        .flat_map(|row| {
            SIZES.into_iter().flat_map(move |n| {
                violations(row, n)
                    .into_iter()
                    .map(move |violation| format!("{}, n={n}: {violation}", row.0))
            })
        })
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A script's error message reaches the host as text: the conversion of a message of any length is paid first, and a
/// call that cannot pay for it ends by its fuel instead.
#[test]
fn an_error_message_exported_to_the_host_is_paid_before_it_is_converted() {
    let row: Row = ("error to the host", TEXT, "error(s, 0)", "#s == n");
    for n in SIZES {
        let full = run(&row, n, AMPLE);
        assert!(
            matches!(&full.outcome, Err(Error::Runtime { message, .. }) if message.len() == n),
            "n={n}: {:?}",
            full.outcome.as_ref().map_err(|e| e.to_string().len())
        );
        assert!(
            full.units >= n as u64,
            "n={n}: the counter sees the conversion"
        );
        assert!(
            full.work <= full.fuel + SLACK,
            "n={n}: {} fuel of work for {}",
            full.work,
            full.fuel
        );
        let short = run(&row, n, full.fuel - 1);
        assert!(
            refused(&short),
            "n={n}: one fuel less ends by fuel: {:?}",
            short.outcome.map_err(|e| e.to_string().len())
        );
        assert!(short.work <= full.fuel + SLACK, "n={n}");
    }
}

/// A set that never closes is malformed when the fuel pays for scanning it to the end, and ends the call by fuel when
/// it does not: the scan that would find the error is never done unpaid.
#[test]
fn a_long_unclosed_set_is_malformed_only_when_its_scan_is_paid() {
    let row: Row = (
        "unclosed set",
        "local p='['..string.rep('x',n)",
        "return string.match('y',p)",
        "#p == n+1",
    );
    let full = run(&row, 16384, AMPLE);
    assert!(
        matches!(&full.outcome, Err(Error::Runtime { message, .. }) if message.contains("missing ']'")),
        "{:?}",
        full.outcome
    );
    let short = run(&row, 16384, full.fuel / 2);
    assert!(refused(&short), "{:?}", short.outcome);
    assert!(
        short.work <= full.fuel / 2 + SLACK,
        "{} fuel of work",
        short.work
    );
}
