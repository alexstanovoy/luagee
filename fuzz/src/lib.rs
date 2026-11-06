//! What the fuzz targets share: the small limits every input runs under, an interpreter that records what a script
//! printed, and an allocator that counts the bytes really held, to check the interpreter's own count against.
#![allow(unsafe_code)]

pub mod generate;
#[path = "../../crates/luagee/tests/support/host_api.rs"]
mod host_api;

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};

use luagee::{Error, Keys, Limit, Limits, Lua, Value};

/// Small bounds, so each input ends fast.
pub const LIMITS: Limits = Limits {
    fuel_per_call: 200_000,
    fuel_per_tick: 400_000,
    memory: 8 << 20,
    string_length: 1 << 20,
    call_depth: 400,
    syntax_depth: 100,
    pattern_depth: 100,
    module_size: 1 << 16,
    scripts_size: 1 << 18,
    module_count: 32,
};

/// The bytes the process really holds beyond what the interpreter counts may not pass this: its count is an estimate
/// (a `Vec` may hold up to twice what it uses), plus the parser's and compiler's passing buffers and the fuzzer's own.
pub const REAL_MEMORY_BOUND: usize = 3 * LIMITS.memory + (16 << 20);

/// The system allocator, counting the bytes held and their peak.
pub struct Counting;

static HELD: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

// SAFETY: every call goes to the system allocator with the caller's arguments unchanged; only counters are added.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: the caller upholds `alloc`'s contract, which `System::alloc` shares.
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            let held = HELD.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
            PEAK.fetch_max(held, Ordering::Relaxed);
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: the caller upholds `dealloc`'s contract, which `System::dealloc` shares.
        unsafe { System.dealloc(pointer, layout) };
        HELD.fetch_sub(layout.size(), Ordering::Relaxed);
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: the caller upholds `realloc`'s contract, which `System::realloc` shares.
        let moved = unsafe { System.realloc(pointer, layout, new_size) };
        if !moved.is_null() {
            if new_size >= layout.size() {
                let held = HELD.fetch_add(new_size - layout.size(), Ordering::Relaxed) + new_size
                    - layout.size();
                PEAK.fetch_max(held, Ordering::Relaxed);
            } else {
                HELD.fetch_sub(layout.size() - new_size, Ordering::Relaxed);
            }
        }
        moved
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// Measures the peak of the bytes really held while `f` runs, above what was held when it started.
pub fn peak_while<T>(f: impl FnOnce() -> T) -> (T, usize) {
    let base = HELD.load(Ordering::Relaxed);
    PEAK.store(base, Ordering::Relaxed);
    let result = f();
    (result, PEAK.load(Ordering::Relaxed) - base)
}

/// An interpreter under [`LIMITS`] whose `print` writes into the returned buffer, and whose modules all read
/// `return ...`.
pub fn interpreter(keys: Keys) -> (Lua, Rc<RefCell<Vec<u8>>>) {
    let mut lua = Lua::new(LIMITS, keys).expect("an interpreter");
    let printed = Rc::new(RefCell::new(Vec::new()));
    let sink = printed.clone();
    lua.set_print(Box::new(move |line| {
        let mut sink = sink.borrow_mut();
        // The printed output is bounded too: a script may print in a loop until its fuel runs out.
        if sink.len() < 1 << 20 {
            sink.extend_from_slice(line);
        }
    }));
    lua.set_loader(|_: &luagee::ModuleName| Some(b"return ...".to_vec()));
    (lua, printed)
}

/// Everything a run of a chunk shows from outside: what it printed, how it ended, and the interpreter's counts.
#[derive(Debug, PartialEq, Eq)]
pub struct Outcome {
    pub printed: Vec<u8>,
    pub ended: String,
    pub fuel_left: u64,
    pub memory: usize,
}

/// Loads and runs a chunk in a fresh interpreter with these keys. The chunk is named `stdin`, as reference
/// interpreters name a chunk they read from their standard input, so error positions read alike.
pub fn run(source: &[u8], keys: Keys) -> Outcome {
    let (mut lua, printed) = interpreter(keys);
    let ended = lua.exec("stdin", source);
    let ended = describe(&lua, ended);
    let printed = printed.borrow().clone();
    Outcome {
        printed,
        ended,
        fuel_left: lua.tick_fuel_left(),
        memory: lua.memory(),
    }
}

/// How a call ended, as text: its results (strings and numbers shown, other values by kind), or its error.
pub fn describe(lua: &Lua, ended: Result<Vec<Value>, Error>) -> String {
    match ended {
        Ok(values) => {
            let shown: Vec<String> = values
                .iter()
                .map(|value| match value {
                    Value::String(_) => {
                        format!(
                            "{:?}",
                            String::from_utf8_lossy(lua.bytes(*value).expect("a string"))
                        )
                    }
                    Value::Number(n) => luagee::number::format(*n),
                    other => format!("{other:?}")
                        .split('(')
                        .next()
                        .unwrap_or("value")
                        .to_owned(),
                })
                .collect();
            format!("ok {}", shown.join(" "))
        }
        Err(error) => format!("{error:?}"),
    }
}

/// The keys the single-run targets use.
pub const KEYS: Keys = Keys {
    hash: [1, 2],
    random: 3,
};

/// Runs a chunk and checks what the sandbox promises whatever it does: no panic (the fuzzer catches one), memory as
/// counted under the ceiling and as really held near it, a collection afterwards that leaves at most the cap (or
/// ends in the memory limit), and an interpreter that still runs a function within a few ticks (a collection the
/// script left under way may take a call's fuel or two first).
pub fn check_run(source: &[u8]) {
    let (lua, _) = interpreter(KEYS);
    check_interpreter(lua, source);
}

/// Exercises the external callback surface under both collection schedules, selected by input bytes.
pub fn check_host(source: &[u8]) {
    let (mut lua, _) = interpreter(KEYS);
    host_api::install(&mut lua).expect("callbacks fit");
    lua.set_gc_stress(match source.first().map(|n| n % 3) {
        Some(1) => luagee::GcStress::Full,
        Some(2) => luagee::GcStress::Incremental,
        _ => luagee::GcStress::Off,
    });
    check_interpreter(lua, source);
}

fn check_interpreter(mut lua: Lua, source: &[u8]) {
    // Loaded first: the input may use up the source limits, which last the interpreter's life.
    let after = lua
        .load(
            "after",
            b"local t = {} for i = 1, 10 do t[i] = i end return #t",
        )
        .expect("compiles");
    lua.pin(after).expect("root fits");
    let (_ended, peak) = peak_while(|| lua.exec("fuzz", source));
    let ceiling = LIMITS.memory + LIMITS.memory / 4;
    assert!(
        lua.memory() <= ceiling,
        "counted {} bytes, past the ceiling {ceiling}",
        lua.memory()
    );
    assert!(
        peak <= REAL_MEMORY_BOUND,
        "really held {peak} bytes at the peak, counting {} at the end",
        lua.memory()
    );
    let mut ran = Err(Error::Limit(Limit::FuelPerCall));
    for _ in 0..10 {
        lua.begin_tick();
        ran = lua.call(after, &[]);
        if !matches!(
            ran,
            Err(Error::Limit(Limit::FuelPerCall | Limit::FuelPerTick))
        ) {
            break;
        }
    }
    match ran {
        Ok(values) => assert_eq!(
            values.len(),
            1,
            "the function after the script returned {values:?}"
        ),
        // What the script left alive may fill the cap: then nothing more fits, which is the cap working.
        Err(Error::Limit(Limit::Memory)) => {}
        Err(error) => panic!("the interpreter broke: {error:?}"),
    }
    match lua.collect() {
        Ok(()) => assert!(
            lua.memory() <= LIMITS.memory,
            "a collection left {} bytes",
            lua.memory()
        ),
        Err(Error::Limit(Limit::Memory)) => {}
        Err(error) => panic!("a collection failed: {error:?}"),
    }
}
