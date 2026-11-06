//! Any bytes as source, only compiled: the parser and compiler end in a function or a syntax error (or a source
//! bound), never a panic, and say the same about the same bytes every time.
#![no_main]

use libfuzzer_sys::fuzz_target;
use luagee::{Error, Keys, Limit, Value};
use luagee_fuzz::interpreter;

const KEYS: Keys = Keys {
    hash: [1, 2],
    random: 3,
};

fn compilation(result: Result<Value, Error>) -> Result<(), Error> {
    result.map(|value| {
        assert!(
            matches!(value, Value::Function(_)),
            "compilation returned {value:?}"
        )
    })
}

fuzz_target!(|data: &[u8]| {
    let (mut lua, _) = interpreter(KEYS);
    let first = lua.load("fuzz", data);
    if let Err(error) = &first {
        assert!(
            matches!(
                error,
                Error::Syntax { .. }
                    | Error::Limit(Limit::ModuleSize | Limit::ScriptsSize | Limit::Memory)
            ),
            "compiling ended in neither a function nor a syntax error: {error:?}"
        );
    }
    let (mut again, _) = interpreter(KEYS);
    let second = again.load("fuzz", data);
    assert_eq!(
        compilation(first),
        compilation(second),
        "the same source compiled differently"
    );
    assert_eq!(lua.memory(), again.memory(), "compilation memory changed");
    assert_eq!(
        lua.tick_fuel_left(),
        again.tick_fuel_left(),
        "compilation fuel changed"
    );
});
