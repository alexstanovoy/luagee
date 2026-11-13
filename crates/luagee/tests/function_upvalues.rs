//! `Limits::function_upvalues`: how many upvalues one function may capture. The default is stock Lua 5.1's and
//! LuaJIT's 60; a host may raise it to run scripts written for a runtime built with more, up to a byte's worth, and
//! a script at the limit and one over it behave the same at every value.

use luagee::{Error, Keys, Limit, Limits, Lua, Value};

/// The most locals one enclosing function declares in these scripts (a function holds at most 200 active).
const LOCALS_PER_LEVEL: usize = 100;

fn lua(function_upvalues: u8) -> Lua {
    lua_with(Limits {
        function_upvalues,
        ..Limits::DEFAULT
    })
}

fn lua_with(limits: Limits) -> Lua {
    Lua::new(
        limits,
        Keys {
            hash: [5, 5],
            random: 5,
        },
    )
    .expect("an interpreter")
}

fn names(count: usize) -> Vec<String> {
    (1..=count).map(|i| format!("u{i}")).collect()
}

/// A script whose innermost code sees `count` locals that enclosing functions declare, `LOCALS_PER_LEVEL` to a
/// function (no function may hold more than 200 locals, and a script needs more than 200 variables to reach the
/// largest limit). `inner` is the innermost function's body, given the names.
fn script(count: usize, inner: impl Fn(&[String]) -> String) -> String {
    let all = names(count);
    let mut levels = all.chunks(LOCALS_PER_LEVEL).enumerate().rev();
    let Some((_, innermost)) = levels.next() else {
        return inner(&all);
    };
    let mut body = format!("local {}\n{}", innermost.join(", "), inner(&all));
    for (depth, level) in levels {
        let below = depth + 1;
        body = format!(
            "local {}\nlocal function level{below}()\n{body}\nend\nreturn level{below}()",
            level.join(", ")
        );
    }
    body
}

/// The line (from 1) of the first line of `source` that is `wanted` once trimmed.
fn line_of(source: &str, wanted: &str) -> usize {
    source
        .lines()
        .position(|line| line.trim() == wanted)
        .unwrap_or_else(|| panic!("no line {wanted:?} in\n{source}"))
        + 1
}

/// A function `f` using all `count` locals.
fn function_using(count: usize) -> String {
    script(count, |names| {
        let uses: Vec<String> = names.iter().map(|name| format!("{name} = 2")).collect();
        format!("local function f()\n{}\nend", uses.join("\n"))
    })
}

fn load_error(lua: &mut Lua, source: &str) -> Option<String> {
    match lua.load("chunk", source.as_bytes()) {
        Ok(_) => None,
        Err(Error::Syntax { line, message, .. }) => {
            let prefix = format!("chunk:{line}: ");
            Some(format!(
                "{line}: {}",
                message.strip_prefix(&prefix).unwrap_or(&message)
            ))
        }
        Err(other) => panic!("expected a syntax error or success, got {other:?}"),
    }
}

/// What a limit of `limit` says of the script that has one upvalue too many in `f`: the line being compiled when the
/// capture past the limit is met (its use of `limit + 1`), and the line `f` begins on.
fn refusal(source: &str, limit: usize) -> String {
    let used = line_of(source, &format!("u{} = 2", limit + 1));
    let defined = line_of(source, "local function f()");
    format!("{used}: function at line {defined} has more than {limit} upvalues")
}

#[test]
fn the_default_is_stock_luas_sixty() {
    assert_eq!(Limits::DEFAULT.function_upvalues, 60);
    assert_eq!(Limits::default().function_upvalues, 60);
}

#[test]
fn by_default_sixty_upvalues_compile_and_sixty_one_are_refused_as_before() {
    let mut lua = lua_with(Limits::DEFAULT);
    assert_eq!(load_error(&mut lua, &function_using(59)), None);
    assert_eq!(load_error(&mut lua, &function_using(60)), None);
    let over = function_using(61);
    assert_eq!(
        load_error(&mut lua, &over),
        Some("63: function at line 2 has more than 60 upvalues".to_owned())
    );
    assert_eq!(load_error(&mut lua, &over), Some(refusal(&over, 60)));
}

#[test]
fn at_any_configured_limit_the_limit_compiles_and_one_more_is_refused_naming_it() {
    for limit in [1u8, 10, 59, 61, 100, 101, 120, 200, 201, 254, 255] {
        let mut lua = lua(limit);
        let at = usize::from(limit);
        assert_eq!(
            load_error(&mut lua, &function_using(at)),
            None,
            "{at} upvalues under a limit of {limit}"
        );
        let over = function_using(at + 1);
        assert_eq!(
            load_error(&mut lua, &over),
            Some(refusal(&over, at)),
            "{} upvalues under a limit of {limit}",
            at + 1
        );
    }
}

#[test]
fn a_limit_of_zero_refuses_the_first_capture() {
    let mut lua = lua(0);
    assert_eq!(
        load_error(&mut lua, "local x\nlocal function f() return x end"),
        Some("2: function at line 2 has more than 0 upvalues".to_owned())
    );
    assert_eq!(
        load_error(&mut lua, "local function f() return y end"),
        None,
        "a global is not an upvalue"
    );
}

#[test]
fn a_function_using_every_upvalue_of_the_largest_limit_reads_and_writes_each_one() {
    let mut lua = lua(u8::MAX);
    let source = script(255, |names| {
        let init: Vec<String> = names
            .iter()
            .enumerate()
            .map(|(i, name)| format!("{name} = {}", i + 1))
            .collect();
        let bumps: Vec<String> = names
            .iter()
            .map(|name| format!("{name} = {name} + 1"))
            .collect();
        let sum: Vec<String> = names.iter().map(|name| format!("s = s + {name}")).collect();
        format!(
            "{}\nlocal function bump()\n{}\nend\nlocal function total()\nlocal s = 0\n{}\nreturn s\nend\nbump()\nreturn total()",
            init.join("\n"),
            bumps.join("\n"),
            sum.join("\n"),
        )
    });
    let results = lua.exec("every", source.as_bytes()).expect("runs");
    // 1 + 2 + ... + 255, and one more each.
    assert_eq!(results, vec![Value::Number(255.0 * 256.0 / 2.0 + 255.0)]);
}

/// A child using `own` of the locals and a grandchild using the `passed` after them.
fn child_and_grandchild(own: usize, passed: usize) -> String {
    script(own + passed, |names| {
        let own_uses: Vec<String> = names[..own].iter().map(|n| format!("{n} = 1")).collect();
        let passed_uses: Vec<String> = names[own..].iter().map(|n| format!("{n} = 1")).collect();
        format!(
            "local function child()\n{}\nlocal function grandchild()\n{}\nend\nend",
            own_uses.join("\n"),
            passed_uses.join("\n"),
        )
    })
}

#[test]
fn a_function_holds_the_upvalues_its_nested_functions_use_as_stock_lua_counts_them() {
    // The child captures what its grandchild uses, so the child holds own + passed.
    for limit in [60usize, 120, 255] {
        let mut lua = lua(limit as u8);
        let half = limit / 2;
        assert_eq!(
            load_error(&mut lua, &child_and_grandchild(half, limit - half)),
            None,
            "limit {limit}"
        );
        let over = child_and_grandchild(half, limit - half + 1);
        let refused = load_error(&mut lua, &over).expect("one more than the child may hold");
        let defined = line_of(&over, "local function child()");
        assert!(
            refused.ends_with(&format!(
                "function at line {defined} has more than {limit} upvalues"
            )),
            "limit {limit}: {refused}"
        );
    }
}

#[test]
fn the_other_function_limits_do_not_move_with_the_upvalue_limit() {
    let mut lua = lua(u8::MAX);
    let locals = |count: usize| format!("local {} = 1", names(count).join(", "));
    assert_eq!(load_error(&mut lua, &locals(200)), None);
    assert_eq!(
        load_error(&mut lua, &locals(201)).as_deref(),
        Some("1: main function has more than 200 local variables")
    );
}

/// A script that makes `count` closures, each capturing `captured` locals, and keeps none of them.
fn closures(captured: usize, count: usize) -> String {
    script(captured, |names| {
        let uses: Vec<String> = names.iter().map(|name| format!("{name} = 2")).collect();
        format!(
            "for i = 1, {count} do\nlocal f = function()\n{}\nend\nend",
            uses.join("\n")
        )
    })
}

/// The fuel one more closure of `captured` upvalues costs in that loop: two runs differing in the closures they
/// make, with the collector not yet started (a few hundred closures hold well under its first threshold).
fn closure_price(captured: usize) -> u64 {
    let spent = |count: usize| {
        let mut lua = lua(u8::MAX);
        lua.exec("closures", closures(captured, count).as_bytes())
            .expect("runs");
        Limits::DEFAULT.fuel_per_tick - lua.tick_fuel_left()
    };
    (spent(200) - spent(100)) / 100
}

#[test]
fn a_closure_costs_its_instruction_an_allocation_and_one_fuel_for_every_eight_upvalues_it_copies() {
    // The loop's own instructions (the closure's and the loop's) and ALLOCATION_FUEL (4) are the price of any closure.
    let plain = closure_price(1);
    assert_eq!(plain, 6, "a closure of one upvalue, loop included");
    assert_eq!(closure_price(7), plain, "under eight upvalues are free");
    assert_eq!(closure_price(8), plain + 1);
    assert_eq!(closure_price(60), plain + 7);
    assert_eq!(closure_price(255), plain + 31);
}

#[test]
fn a_closure_whose_copy_is_not_paid_is_refused_before_it_is_made() {
    let source = closures(255, 1);
    let mut priced = lua(u8::MAX);
    priced.exec("priced", source.as_bytes()).expect("runs");
    let price = Limits::DEFAULT.fuel_per_tick - priced.tick_fuel_left();
    let mut short = lua_with(Limits {
        function_upvalues: u8::MAX,
        fuel_per_call: price - 1,
        fuel_per_tick: price - 1,
        ..Limits::DEFAULT
    });
    let ended = short.exec("short", source.as_bytes());
    assert!(
        matches!(ended, Err(Error::Limit(Limit::FuelPerCall))),
        "{ended:?}"
    );
}

#[test]
fn closures_with_the_most_upvalues_count_as_memory_and_are_collected() {
    let limits = Limits {
        function_upvalues: u8::MAX,
        memory: 4 << 20,
        ..Limits::DEFAULT
    };
    let mut lua = lua_with(limits);
    // Kept alive in a table, they run into the cap as a limit, not as a panic.
    let keep = script(255, |names| {
        let uses: Vec<String> = names.iter().map(|name| format!("{name} = 2")).collect();
        format!(
            "local kept = {{}}\nfor i = 1, 100000 do\nkept[i] = function()\n{}\nend\nend",
            uses.join("\n")
        )
    });
    let ended = lua.exec("keep", keep.as_bytes());
    assert!(
        matches!(ended, Err(Error::Limit(Limit::Memory))),
        "{ended:?}"
    );
    // Dropped, the same closures are garbage and the memory comes back.
    lua.begin_tick();
    lua.collect().expect("collects");
    assert!(lua.memory() < 1 << 20, "{} bytes left", lua.memory());
    lua.begin_tick();
    lua.exec("garbage", closures(255, 5000).as_bytes())
        .expect("garbage closures are reclaimed under the cap");
    assert!(lua.memory() < 4 << 20);
}
