//! Any bytes as source, only compiled: the parser and compiler end in a function or a syntax error (or a source
//! bound), never a panic, and say the same about the same bytes every time.
#![no_main]

use libfuzzer_sys::fuzz_target;
use luagee::{Error, Keys, Limit};
use luagee_fuzz::interpreter;

const KEYS: Keys = Keys { hash: [1, 2], random: 3 };

fuzz_target!(|data: &[u8]| {
    let (mut lua, _) = interpreter(KEYS);
    let first = lua.load("fuzz", data);
    if let Err(error) = &first {
        assert!(
            matches!(error, Error::Syntax { .. } | Error::Limit(Limit::ModuleSize | Limit::ScriptsSize | Limit::Memory)),
            "compiling ended in neither a function nor a syntax error: {error:?}"
        );
    }
    let (mut again, _) = interpreter(KEYS);
    let second = again.load("fuzz", data);
    assert_eq!(format!("{first:?}"), format!("{second:?}"), "the same source compiled differently");
});
