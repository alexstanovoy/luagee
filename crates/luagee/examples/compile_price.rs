//! Measures what compiling costs a token, for source shaped to make it dear, and prints the nanoseconds a fuel buys
//! under the price in `src/parser.rs`: `docs/fuel_audit.md` quotes its table.
//!
//!     cargo run --release -p luagee --example compile_price
//!
//! Each shape is a unit of source repeated until it is about `SIZE` bytes (default 400000), with the tokens one unit
//! holds. The compile is timed through `Lua::load` (best of `REPEATS`, default 5); the fuel is what `load` from a
//! script spends on the same source, found by running it with a fuel budget and reading what is left.

use std::time::Instant;

use luagee::{Keys, Limits, Lua};

/// (name, one unit of source, tokens in a unit). A `{}` in a unit is a number that differs from unit to unit.
const SHAPES: &[(&str, &str, usize)] = &[
    ("empty statements `;`", ";", 1),
    ("`a=a `", "a=a ", 3),
    ("global reads `_=g`", "_=g\n", 3),
    ("nested parentheses", "x=((((((((((1))))))))))\n", 23),
    ("sum chain", "x=1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1\n", 33),
    ("concat chain", "x=a..a..a..a..a..a..a..a..a..a..a..a\n", 25),
    ("power chain", "x=2^2^2^2^2^2^2^2\n", 17),
    ("short strings", "x={'a','b','c','d','e','f','g','h'}\n", 20),
    ("numerals", "x={1,2,3,4,5,6,7,8,9,1,2,3,4,5,6,7}\n", 36),
    ("tiny functions", "function f() end\n", 6),
    ("function values", "f=function()end\n", 6),
    ("local functions", "do local function f()end end\n", 9),
    ("distinct globals", "v{}=w{}\n", 3),
    ("distinct strings", "x='s{}'\n", 3),
    ("distinct numerals", "x={}.5\n", 3),
    ("calls", "f()f()f()f()f()f()f()f()\n", 24),
    ("method calls", "a:b()a:b()a:b()a:b()\n", 20),
    ("field chains", "x=a.b.c.d.e.f.g.h\n", 17),
    ("empty blocks", "do end do end do end do end\n", 8),
    ("while and if", "while a do end if a then end\n", 12),
    ("nested tables", "x={{{{{{}}}}}}\n", 15),
    ("distinct labels", "::l{}::\n", 3),
    ("return values", "do return 1,2,3,4,5,6,7,8 end\n", 20),
    ("numeric for", "for i=1,2 do end\n", 8),
    ("comment lines", "--xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx\n", 0),
    (
        "long string",
        "x=[==[xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx]==]\n",
        3,
    ),
    ("many parameters", "function f(a,b,c,d,e,f,g,h) end\n", 19),
    ("many locals", "do local a,b,c,d,e,f,g,h end\n", 19),
    ("keyed fields", "x={a=1,b=2,c=3,d=4,e=5,f=6,g=7,h=8}\n", 40),
    (
        "bracketed fields",
        "x={[1]=1,[2]=2,[3]=3,[4]=4,[5]=5,[6]=6}\n",
        42,
    ),
    ("string escapes", "x='\\n\\n\\n\\n\\n\\n\\n\\n'\n", 3),
    (
        "hex floats",
        "x={0x1p4,0x2p4,0x3p4,0x4p4,0x5p4,0x6p4}\n",
        16,
    ),
    ("method definitions", "function a.b:c() end\n", 10),
    (
        "elseif chains",
        "if a then elseif b then elseif c then elseif d then end\n",
        17,
    ),
    ("unary chains", "x=not not not not not not not not a\n", 11),
    ("comparisons", "x=a<b==c>d~=e<=f>=g\n", 14),
    ("repeat until", "repeat until a\n", 4),
    ("varargs", "x={...,...,...,...,...,...,...,...}\n", 20),
    ("gotos", "goto l{}\n::l{}::\n", 6),
    (
        "ordinary code",
        "do local function add(a, b)\n  if a > b then return a - b end\n  return a + b -- sum\nend\nlocal t = { add(1, 2), name = 'x' }\nend\n",
        43,
    ),
];

/// `numerator / denominator` to a tenth, as text (the crate's lints keep floating point out of everything but `number`).
fn tenths(numerator: u128, denominator: u128) -> String {
    let tenths = numerator * 10 / denominator;
    format!("{}.{}", tenths / 10, tenths % 10)
}

fn setting(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn interpreter(fuel: u64) -> Lua {
    let limits = Limits::DEFAULT
        .with_fuel_per_call(fuel)
        .with_fuel_per_tick(fuel)
        .with_memory(1 << 30)
        .with_module_size(1 << 26)
        .with_scripts_size(1 << 28);
    Lua::new(
        limits,
        Keys {
            hash: [1, 2],
            random: 3,
        },
    )
    .expect("an interpreter")
}

/// The fuel a script's `load` of `source` spends.
fn fuel_of_load(source: &str) -> u64 {
    let budget = 1 << 50;
    let mut lua = interpreter(budget);
    let script =
        format!("local src = [=====[\n{source}]=====] local f, e = load(src) assert(f, e)");
    lua.exec("price", script.as_bytes()).expect("loads");
    let with_load = budget - lua.tick_fuel_left();
    let mut lua = interpreter(budget);
    let script = format!("local src = [=====[\n{source}]=====] local f, e = src, nil");
    lua.exec("base", script.as_bytes()).expect("runs");
    with_load - (budget - lua.tick_fuel_left())
}

/// Real source: whole scripts of the conformance corpus, each in its own block, up to about `size` bytes.
fn typical(size: usize) -> String {
    let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/conformance");
    let mut paths: Vec<_> = std::fs::read_dir(folder)
        .expect("the conformance corpus")
        .map(|entry| entry.expect("an entry").path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "lua"))
        .collect();
    // Smallest first, so that a small size still holds a script; a script too large for what is left is passed over.
    paths.sort_by_key(|path| std::fs::metadata(path).expect("a script").len());
    let mut source = String::new();
    for path in &paths {
        let text = std::fs::read_to_string(path).expect("a script");
        let block = format!("do\n{text}\nend\n");
        let compiles = interpreter(1 << 50).load("block", block.as_bytes()).is_ok();
        if compiles && source.len() + block.len() <= size {
            source.push_str(&block);
        }
    }
    source
}

fn main() {
    let size = setting("SIZE", 400_000);
    let repeats = setting("REPEATS", 5);
    println!(
        "{:<24} {:>9} {:>9} {:>9} {:>8} {:>8}",
        "shape", "bytes", "tokens", "fuel", "ns/token", "ns/fuel"
    );
    for &(name, unit, tokens) in SHAPES {
        let count = size / unit.len();
        let source: String = (0..count)
            .map(|i| unit.replace("{}", &i.to_string()))
            .collect();
        let total_tokens = tokens * count;
        let fuel = fuel_of_load(&source);
        let mut best = u128::MAX;
        for _ in 0..repeats {
            let mut lua = interpreter(1 << 50);
            let start = Instant::now();
            let loaded = lua.load("shape", source.as_bytes());
            let elapsed = start.elapsed().as_nanos();
            assert!(loaded.is_ok(), "{name}: {loaded:?}");
            best = best.min(elapsed);
        }
        let per_token = if total_tokens == 0 {
            "-".to_owned()
        } else {
            tenths(best, total_tokens as u128)
        };
        println!(
            "{name:<24} {:>9} {:>9} {:>9} {:>8} {:>8}",
            source.len(),
            total_tokens,
            fuel,
            per_token,
            tenths(best, fuel as u128)
        );
    }
    println!();
    println!(
        "{:<24} {:>9} {:>9} {:>9} {:>8}",
        "typical source", "bytes", "before", "fuel", "ns/fuel"
    );
    for size in [1 << 10, 100 << 10] {
        let source = typical(size);
        let fuel = fuel_of_load(&source);
        let mut best = u128::MAX;
        for _ in 0..repeats {
            let mut lua = interpreter(1 << 50);
            let start = Instant::now();
            let loaded = lua.load("typical", source.as_bytes());
            let elapsed = start.elapsed().as_nanos();
            assert!(loaded.is_ok(), "typical: {loaded:?}");
            best = best.min(elapsed);
        }
        println!(
            "{:<24} {:>9} {:>9} {:>9} {:>8}",
            "conformance scripts",
            source.len(),
            source.len() / 4,
            fuel,
            tenths(best, fuel as u128)
        );
    }
}
