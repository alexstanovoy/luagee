//! Lua programs made from the fuzzer's bytes (they always parse, so the run, not the parser, gets the coverage), run
//! with the checks of `run_bytes`.
#![no_main]

use libfuzzer_sys::arbitrary::Unstructured;
use libfuzzer_sys::fuzz_target;
use luagee_fuzz::check_run;
use luagee_fuzz::generate::{Dialect, program};

fuzz_target!(|data: &[u8]| {
    let mut u = Unstructured::new(data);
    if let Ok(source) = program(&mut u, Dialect::Any) {
        check_run(source.as_bytes());
    }
});
