//! Fuel is proportional to the work it pays for, and paid before the work. For every operation whose work a script
//! sizes, at three sizes: the counted work stays within the fuel spent; a call given half that fuel is refused having
//! done no more work than that half pays for, and leaves the state as it was; the exact fuel suffices and one less is
//! refused. Work is counted at the price the operation charges for it (see `Work::per_fuel`).

use super::Work;
use crate::{Error, Keys, Limit, Limits, Lua, Value};

/// Every kind of work, in the order of its discriminant.
const ALL: [Work; 6] = [
    Work::Slots,
    Work::Bytes,
    Work::Collector,
    Work::Steps,
    Work::Names,
    Work::Cells,
];

/// The units of a kind of work one fuel pays for: the price the operations charge for it.
const fn per_fuel(work: Work) -> u64 {
    match work {
        Work::Slots => crate::vm::VALUES_PER_FUEL as u64,
        Work::Bytes => 16,
        Work::Collector => crate::vm::GC_OBJECTS_PER_FUEL as u64,
        Work::Steps => 1,
        Work::Cells => crate::vm::VALUES_PER_FUEL as u64,
        // A fuel pays for four source bytes; a name use is two bytes at least and looks its name up four times.
        Work::Names => 4,
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

/// A function holding 60 upvalues (the default limit) and then `n` one-line statements reading a global: every use of a
/// name is resolved against the function's upvalues and its enclosing function's locals.
const UPVALUES_THEN_READS: &str = "local names, uses = {}, {} for i = 1, 60 do names[i] = 'u'..i uses[i] = 'u'..i..' = 2' end
    local src = 'local '..table.concat(names, ',')..'\\nlocal function f()\\nlocal _\\n'..table.concat(uses, '\\n')..'\\n'..string.rep('_ = g\\n', n)..'end'";
/// 40 functions nested, each with 150 locals still active in the next, and `n` one-line statements reading a global in
/// the innermost: every use of a name is resolved against all of them.
const NESTED_LOCALS_THEN_READS: &str = "local src = '' for level = 1, 40 do local names = {} for i = 1, 150 do names[i] = 'v'..level..'_'..i end
    src = src..'local '..table.concat(names, ',')..'\\nlocal function l'..level..'()\\n' end
    src = src..'local _\\n'..string.rep('_ = g\\n', n)..string.rep('end\\n', 40)";
/// A block of `n` labels.
const LABELS: &str = "local labels = {} for i = 1, n do labels[i] = '::l'..i..'::' end local src = table.concat(labels, '\\n')";

/// 56 upvalues (a closure of 56 pays 7 fuel for its cells) and `n` closures that each capture all of them.
const CLOSURES_OF_56_UPVALUES: &str = "local u1,u2,u3,u4,u5,u6,u7,u8,u9,u10,u11,u12,u13,u14,u15,u16,u17,u18,u19,u20,u21,u22,u23,u24,u25,u26,u27,u28,u29,u30,u31,u32,u33,u34,u35,u36,u37,u38,u39,u40,u41,u42,u43,u44,u45,u46,u47,u48,u49,u50,u51,u52,u53,u54,u55,u56
    local function make() for i = 1, n do local f = function() u1=2 u2=2 u3=2 u4=2 u5=2 u6=2 u7=2 u8=2 u9=2 u10=2 u11=2 u12=2 u13=2 u14=2 u15=2 u16=2 u17=2 u18=2 u19=2 u20=2 u21=2 u22=2 u23=2 u24=2 u25=2 u26=2 u27=2 u28=2 u29=2 u30=2 u31=2 u32=2 u33=2 u34=2 u35=2 u36=2 u37=2 u38=2 u39=2 u40=2 u41=2 u42=2 u43=2 u44=2 u45=2 u46=2 u47=2 u48=2 u49=2 u50=2 u51=2 u52=2 u53=2 u54=2 u55=2 u56=2 end end end";

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
    ("load, a reader's piece then a number", "local s=string.rep('x',n) local k=0", "return load(function() k=k+1 if k==1 then return s end return 1 end)", "#s == n"),
    ("load, reads under 60 upvalues", UPVALUES_THEN_READS, "return load(src)", "#src > n"),
    ("load, reads under 40 nested functions of 150 locals", NESTED_LOCALS_THEN_READS, "return load(src)", "#src > n"),
    ("closures of 56 upvalues", CLOSURES_OF_56_UPVALUES, "make()", "true"),
    ("load, a block of labels", LABELS, "return load(src)", "#src > n"),
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
/// it does not: the scan that finds the error is paid, so that catching the error and trying again costs in
/// proportion to the set.
#[test]
fn a_long_unclosed_set_is_malformed_only_when_its_scan_is_paid() {
    let row: Row = (
        "unclosed set",
        "local p='['..string.rep('x',n)",
        "return pcall(string.match,'y',p)",
        "#p == n+1",
    );
    for n in SIZES {
        let full = run(&row, n, AMPLE);
        assert!(full.outcome.is_ok(), "n={n}: {:?}", full.outcome);
        assert!(
            full.units >= n as u64,
            "n={n}: the counter sees the scan, {} units",
            full.units
        );
        assert!(
            full.fuel >= (n / 16) as u64,
            "n={n}: the scan of {n} bytes costs {} fuel",
            full.fuel
        );
        assert!(
            full.work <= full.fuel + SLACK,
            "n={n}: {} fuel of work for {}",
            full.work,
            full.fuel
        );
        let short = run(&row, n, full.fuel / 2);
        assert!(refused(&short), "n={n}: {:?}", short.outcome);
        assert!(
            short.work <= full.fuel / 2 + SLACK,
            "n={n}: {} fuel of work before refusing",
            short.work
        );
    }
}

/// A chunk `load` or `loadstring` refuses before compiling it (bytecode) costs the same at any length: it is shared,
/// never copied.
#[test]
fn a_refused_bytecode_chunk_costs_the_same_at_any_length() {
    for operation in ["return loadstring(s)", "return load(s)"] {
        let row: Row = (
            "bytecode",
            "local s='\\27'..string.rep('x',n)",
            operation,
            "#s == n+1",
        );
        let fuels = SIZES.map(|n| run(&row, n, AMPLE).fuel);
        assert!(
            fuels.iter().all(|&fuel| fuel == fuels[0]),
            "{operation}: {fuels:?}"
        );
    }
}

/// Matching a byte against a set scans the set twice: once to find its `]`, once to test the byte; both are counted
/// where they happen (and paid). The pattern is anchored: one position, one scan of each kind.
#[test]
fn both_scans_of_a_pattern_set_are_counted() {
    let row: Row = (
        "set",
        "local p='^['..string.rep('x',n)..']'",
        "return string.match('y',p)",
        "#p == n+3",
    );
    for n in SIZES {
        let full = run(&row, n, AMPLE);
        assert!(full.units >= 2 * n as u64, "n={n}: {} units", full.units);
        assert!(
            full.work <= full.fuel + SLACK,
            "n={n}: {} fuel of work for {}",
            full.work,
            full.fuel
        );
    }
}

/// A name used deep inside nested functions is taken as an upvalue by each of them on the way: that is as many
/// upvalues as functions times names, however short the source, so each is paid when it is taken (`CAPTURE_FUEL`), and a
/// compile whose call cannot pay ends there, having taken no more than it paid for.
#[test]
fn upvalues_taken_through_nested_functions_are_paid_one_by_one_before_they_are_taken() {
    const NAMES: u64 = 30;
    let row: Row = (
        "captures",
        "local names, uses = {}, {} for i = 1, 30 do names[i] = 'a'..i uses[i] = '_ = a'..i end
        local src = 'local '..table.concat(names, ',')..'\\n'
        for level = 1, n do src = src..'local function f'..level..'()\\n' end
        src = src..'local _\\n'..table.concat(uses, '\\n')..'\\n'..string.rep('end\\n', n)",
        "return load(src)",
        "#src > n",
    );
    for depth in [8usize, 32, 90] {
        let full = run(&row, depth, AMPLE);
        assert!(full.outcome.is_ok(), "depth {depth}: {:?}", full.outcome);
        let upvalues = NAMES * depth as u64;
        assert!(
            full.units >= upvalues * super::super::compiler::CAPTURE_FUEL,
            "depth {depth}: the counter sees every upvalue taken, {} units",
            full.units
        );
        assert!(
            full.fuel >= upvalues * super::super::compiler::CAPTURE_FUEL,
            "depth {depth}: {} fuel for {upvalues} upvalues",
            full.fuel
        );
        assert!(
            full.work <= full.fuel + SLACK,
            "depth {depth}: {} fuel of work for {}",
            full.work,
            full.fuel
        );
        let short = run(&row, depth, full.fuel / 2);
        assert!(refused(&short), "depth {depth}: {:?}", short.outcome);
        assert!(
            short.kept,
            "depth {depth}: a refused compile changed the state"
        );
        assert!(
            short.work <= full.fuel / 2 + SLACK,
            "depth {depth}: {} fuel of work done before refusing at {}",
            short.work,
            full.fuel / 2
        );
        assert!(
            refused(&run(&row, depth, full.fuel - 1)),
            "depth {depth}: one fuel less is refused"
        );
    }
}

/// A closure's price is paid before its cells are copied: the least fuel that lets a call copy them is the price of
/// everything up to the closure and the closure's own, not less. (The rows above hold the sums; this holds the order,
/// which a sum cannot see.)
#[test]
fn a_closure_the_call_cannot_pay_for_copies_no_cells() {
    let row: Row = ("one closure", CLOSURES_OF_56_UPVALUES, "make()", "true");
    let full = run(&row, 1, AMPLE);
    assert_eq!(full.units, 56, "a closure of 56 upvalues copies 56 cells");
    let copies = |budget: u64| run(&row, 1, budget).units;
    let least = (0..=full.fuel)
        .find(|&budget| copies(budget) == 56)
        .expect("the full fuel copies them");
    // What the call pays after the closure: the loop's next test, and the returns of `make` and the function around it.
    assert_eq!(
        full.fuel - least,
        3,
        "the closure is the last thing the least fuel pays for, three fuel short of the whole call"
    );
    assert_eq!(copies(least - 1), 0, "one fuel less copies nothing");
}

/// Each token is paid for as the lexer reads it (`TOKEN_FUEL`), after the quarter fuel a byte that is paid up front: a
/// budget that covers the bytes and five tokens reads five and stops at the sixth, having read no more than it paid for,
/// however long the rest of the source is.
#[test]
fn a_compile_whose_fuel_ends_stops_reading_tokens_where_it_ends() {
    use crate::parser::TOKEN_FUEL;
    let row: Row = (
        "tokens",
        "local src = string.rep(';', n)",
        "return load(src)",
        "#src == n",
    );
    for n in SIZES {
        // A token a byte, and the end of the source.
        let tokens = n as u64 + 1;
        let full = run(&row, n, AMPLE);
        assert!(full.outcome.is_ok(), "n={n}: {:?}", full.outcome);
        // Besides the tokens, the one step that looks for the chunk's trailing labels.
        assert_eq!(
            full.units,
            TOKEN_FUEL * tokens + 1,
            "n={n}: every token is counted"
        );
        let before_tokens = full.fuel - TOKEN_FUEL * tokens - 1;
        for read in [0u64, 5, tokens / 2] {
            let budget = before_tokens + TOKEN_FUEL * read + (TOKEN_FUEL - 1);
            let short = run(&row, n, budget);
            assert!(refused(&short), "n={n}, {read} tokens: {:?}", short.outcome);
            assert_eq!(
                short.units,
                TOKEN_FUEL * read,
                "n={n}: the compile reads the tokens it paid for and no more"
            );
            assert!(short.kept, "n={n}: a refused compile changed the state");
        }
    }
}
