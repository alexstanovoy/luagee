//! Any bytes as source, compiled and run in the sandbox with small limits: nothing panics the host, every run ends
//! in a result, an error or a limit, the interpreter's count of its memory stays under the cap (and the bytes it
//! really holds near it), and the interpreter stays usable after whatever the script did.
#![no_main]

use libfuzzer_sys::fuzz_target;
use luagee_fuzz::check_run;

fuzz_target!(|data: &[u8]| check_run(data));
