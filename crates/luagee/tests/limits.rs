//! What `Lua::new` does with limits: it refuses those that cannot hold an interpreter, with an error that names the
//! field, and accepts every other value (a script meets it as its own bound).

use luagee::{Error, Keys, Limit, Limits, LimitsError, Lua};

const KEYS: Keys = Keys {
    hash: [1, 2],
    random: 3,
};

/// Message-building code that touches the library, strings and tables.
const SMALL_SCRIPT: &[u8] = b"local t = {} for i = 1, 10 do t[i] = tostring(i) end \
    local ok, message = pcall(function() local x = nil; return x.y end) \
    return #table.concat(t) + #message";

#[test]
fn a_memory_below_the_minimum_is_refused_naming_the_field() {
    let refused = Lua::new(Limits::DEFAULT.with_memory(0), KEYS).err();
    assert_eq!(
        refused,
        Some(Error::InvalidLimits(LimitsError::Memory {
            given: 0,
            minimum: Limits::MIN_MEMORY
        }))
    );
    assert_eq!(
        refused.expect("an error").to_string(),
        "invalid limits: memory is 0 bytes, but an interpreter needs at least 65536"
    );
}

#[test]
fn a_string_length_below_the_minimum_is_refused_naming_the_field() {
    let refused = Lua::new(Limits::DEFAULT.with_string_length(0), KEYS).err();
    assert_eq!(
        refused.expect("an error").to_string(),
        "invalid limits: string_length is 0 bytes, but an interpreter needs at least 16"
    );
}

#[test]
fn one_byte_less_than_each_minimum_is_refused_and_the_minimum_itself_runs_a_script() {
    let refused = Lua::new(Limits::DEFAULT.with_memory(Limits::MIN_MEMORY - 1), KEYS);
    assert!(matches!(
        refused,
        Err(Error::InvalidLimits(LimitsError::Memory { .. }))
    ));
    let refused = Lua::new(
        Limits::DEFAULT.with_string_length(Limits::MIN_STRING_LENGTH - 1),
        KEYS,
    );
    assert!(matches!(
        refused,
        Err(Error::InvalidLimits(LimitsError::StringLength { .. }))
    ));
    let smallest = Limits::DEFAULT
        .with_memory(Limits::MIN_MEMORY)
        .with_string_length(Limits::MIN_STRING_LENGTH);
    let mut lua = Lua::new(smallest, KEYS).expect("the smallest limits make an interpreter");
    // The least memory leaves a script little room, but the interpreter is whole.
    assert_eq!(
        lua.exec("tiny", b"return 1"),
        Ok(vec![luagee::Value::Number(1.0)])
    );
    // Its own error, whose message is longer than the string bound, is still raised.
    assert!(matches!(
        lua.exec("failing", b"local x x.y = 1"),
        Err(Error::Runtime { .. })
    ));
    assert!(lua.memory() > 0);
}

#[test]
fn validate_agrees_with_construction() {
    assert_eq!(Limits::DEFAULT.validate(), Ok(()));
    assert!(Limits::DEFAULT.with_memory(1).validate().is_err());
    assert!(Limits::DEFAULT.with_string_length(1).validate().is_err());
}

#[test]
fn every_other_zero_is_accepted_and_the_script_meets_its_own_bound() {
    let zeroed = [
        ("fuel_per_call", Limits::DEFAULT.with_fuel_per_call(0)),
        ("fuel_per_tick", Limits::DEFAULT.with_fuel_per_tick(0)),
        ("call_depth", Limits::DEFAULT.with_call_depth(0)),
        ("syntax_depth", Limits::DEFAULT.with_syntax_depth(0)),
        ("pattern_depth", Limits::DEFAULT.with_pattern_depth(0)),
        (
            "function_upvalues",
            Limits::DEFAULT.with_function_upvalues(0),
        ),
        ("module_size", Limits::DEFAULT.with_module_size(0)),
        ("scripts_size", Limits::DEFAULT.with_scripts_size(0)),
        ("module_count", Limits::DEFAULT.with_module_count(0)),
    ];
    for (field, limits) in zeroed {
        let mut lua = Lua::new(limits, KEYS)
            .unwrap_or_else(|error| panic!("{field} of 0 is a bound, not an error: {error}"));
        // Whatever the script does, the host gets an Error, not a panic.
        let _ = lua.exec("zero", SMALL_SCRIPT);
    }
    let mut lua = Lua::new(Limits::DEFAULT.with_fuel_per_call(0), KEYS).expect("an interpreter");
    assert_eq!(
        lua.exec("none", b"return 1"),
        Err(Error::Limit(Limit::FuelPerCall))
    );
}

#[test]
fn the_largest_values_are_accepted_and_run() {
    let largest = Limits::DEFAULT
        .with_fuel_per_call(u64::MAX)
        .with_fuel_per_tick(u64::MAX)
        .with_memory(usize::MAX)
        .with_string_length(usize::MAX)
        .with_call_depth(u32::MAX)
        .with_module_size(usize::MAX)
        .with_scripts_size(usize::MAX)
        .with_module_count(u32::MAX);
    let mut lua = Lua::new(largest, KEYS).expect("an interpreter");
    lua.exec("largest", SMALL_SCRIPT).expect("runs");
}
