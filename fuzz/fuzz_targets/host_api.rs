//! Fuzzed scripts call a host capability table: arguments, typed userdata, raw tables, roots and nested calls.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| luagee_fuzz::check_host(data));
