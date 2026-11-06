//! The smallest host embedding luagee, for measuring what the interpreter adds to a wasm module: one exported
//! function that runs a script and returns its result. `tools/wasm.sh size` builds it for wasm32-unknown-unknown.

use luagee::{Keys, Limits, Lua, Value};

const SCRIPT: &[u8] =
    b"local t = {} for i = 1, 10 do t[i] = string.rep('x', i) end return #table.concat(t)";

/// The script's result, or -1 when it fails.
#[unsafe(no_mangle)]
pub extern "C" fn run(hash: u64) -> f64 {
    let Ok(mut lua) = Lua::new(
        Limits::DEFAULT,
        Keys {
            hash: [hash, hash],
            random: hash,
        },
    ) else {
        return -1.0;
    };
    match lua.exec("embed", SCRIPT).as_deref() {
        Ok([Value::Number(n)]) => *n,
        _ => -1.0,
    }
}
