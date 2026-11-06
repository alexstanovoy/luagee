//! Lua source from a stranger, compiled and run in the sandbox with small limits: nothing panics the host, nothing
//! runs past its fuel, and the interpreter stays usable after whatever the script did.
#![no_main]

use libfuzzer_sys::fuzz_target;
use luagee::{Limits, Lua};

/// Small bounds, so each input ends fast.
const LIMITS: Limits = Limits {
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

fuzz_target!(|data: &[u8]| {
    let mut lua = Lua::new(LIMITS, 1).expect("an interpreter");
    lua.set_loader(|_: &luagee::ModuleName| Some(b"return ...".to_vec()));
    // Loaded first: the input may use up the source limits, which last the interpreter's life.
    let after = lua
        .load(
            "after",
            b"local t = {} for i = 1, 10 do t[i] = i end return #t",
        )
        .expect("compiles");
    lua.pin(after);
    // Any outcome is fine; only a panic, a hang or a blowup is a finding.
    let _ = lua.exec("fuzz", data);
    // The interpreter still runs a function after any outcome.
    lua.begin_tick();
    let after = lua.call(after, &[]);
    assert!(
        matches!(after, Ok(ref v) if v.len() == 1),
        "the interpreter broke: {after:?}"
    );
});
