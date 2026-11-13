//! Fuel is proportional to the work it pays for, and paid before the work: for every operation whose work a script
//! sizes, at three sizes, the counted work stays within the fuel spent, a call given half that fuel is refused having
//! done at most half the work, the exact fuel suffices and one less is refused.

use crate::{Error, Keys, Limit, Limits, Lua, Value};

/// Rounding: each kind of work may round up by one fuel.
const SLACK: u64 = 2;
const SIZES: [usize; 3] = [64, 1024, 16384];

/// An operation whose work grows with `n`: `prepare` (Lua with `n` bound) sets up the state, `operation` is the body of
/// the function measured, and `unchanged` an expression that is true while a refused call left the state as it was.
struct Row {
    name: &'static str,
    prepare: &'static str,
    operation: &'static str,
    unchanged: &'static str,
}

const ROWS: &[Row] = &[
    Row {
        name: "table.maxn over a cleared array",
        prepare: "local t={} for i=1,n do t[i]=i end for i=1,n do t[i]=nil end",
        operation: "return table.maxn(t)",
        unchanged: "next(t) == nil",
    },
    Row {
        name: "table.maxn over removed hash keys",
        prepare: "local t={} for i=1,n do t[-i]=i end for i=1,n do t[-i]=nil end",
        operation: "return table.maxn(t)",
        unchanged: "next(t) == nil",
    },
    Row {
        name: "table.maxn over live hash keys",
        prepare: "local t={} for i=1,n do t[i+0.5]=i end",
        operation: "return table.maxn(t)",
        unchanged: "t[1.5] == 1",
    },
];

fn interpreter() -> Lua {
    Lua::new(
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
    .expect("state")
}

/// A prepared interpreter with the measured function and the state check, collected, its counts reset.
fn prepared(row: &Row, n: usize) -> (Lua, Value, Value) {
    let mut lua = interpreter();
    let source = format!(
        "local n = {n} {} return function() {} end, function() return {} end",
        row.prepare, row.operation, row.unchanged
    );
    let functions = lua.exec(row.name, source.as_bytes()).expect("prepares");
    for &function in &functions {
        lua.pin(function).expect("root fits");
    }
    lua.collect().expect("collects");
    super::take();
    (lua, functions[0], functions[1])
}

/// Runs the operation with `budget` fuel for the call: its outcome, the work it did in fuel and in units, the fuel spent.
fn run(row: &Row, n: usize, budget: u64) -> (Result<(), Error>, u64, u64, u64) {
    let (mut lua, operation, unchanged) = prepared(row, n);
    lua.vm().limits.fuel_per_call = budget;
    lua.begin_tick();
    let outcome = lua.call(operation, &[]).map(drop);
    let (work, units) = super::take();
    let spent = (1 << 40) - lua.tick_fuel_left();
    if outcome.is_err() {
        lua.vm().limits.fuel_per_call = 1 << 40;
        lua.begin_tick();
        let check = lua.call(unchanged, &[]).expect("the state check runs");
        assert_eq!(
            check,
            [Value::Boolean(true)],
            "{}, n={n}: a refused call left the state as it was",
            row.name
        );
    }
    (outcome, work, units, spent)
}

/// What one row breaks at one size, if anything.
fn violations(row: &Row, n: usize) -> Vec<String> {
    let refused =
        |outcome: &Result<(), Error>| matches!(outcome, Err(Error::Limit(Limit::FuelPerCall)));
    let mut found = Vec::new();
    let (outcome, work, units, fuel) = run(row, n, 1 << 40);
    if let Err(error) = outcome {
        return vec![format!("fails with ample fuel: {error}")];
    }
    if units < n as u64 {
        found.push(format!("the counter misses the work: {units} units"));
    }
    if work > fuel + SLACK {
        found.push(format!("{work} fuel of work for {fuel} fuel"));
    }
    let half = fuel / 2;
    let (outcome, refused_work, ..) = run(row, n, half);
    if !refused(&outcome) {
        found.push(format!(
            "half the fuel ({half}) is not refused: {outcome:?}"
        ));
    }
    if refused_work > half + SLACK {
        found.push(format!(
            "{refused_work} fuel of work done before refusing at {half}"
        ));
    }
    if let (Err(error), ..) = run(row, n, fuel) {
        found.push(format!("the exact fuel ({fuel}) does not suffice: {error}"));
    }
    if !refused(&run(row, n, fuel - 1).0) {
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
                    .map(move |violation| format!("{}, n={n}: {violation}", row.name))
            })
        })
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
