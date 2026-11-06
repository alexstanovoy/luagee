//! Runs a Lua file in the sandbox: `print` goes to stdout, `require` reads modules next to the file, and an error
//! goes to stderr with exit code 1. The fuel per call is raised so that long test programs finish; `LUA_FUEL` and
//! `LUA_MEMORY` (bytes) set them instead.
//!
//!     cargo run --release -p luagee --example lua -- FILE.lua

use std::io::Write as _;
use std::path::PathBuf;

use luagee::{FolderLoader, Limits, Lua};

fn main() {
    let path = PathBuf::from(std::env::args().nth(1).expect("usage: lua FILE.lua"));
    let source = std::fs::read(&path).expect("read the script");
    let folder = path.parent().map(PathBuf::from).unwrap_or_default();
    let setting = |name: &str, default: u64| {
        std::env::var(name)
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(default)
    };
    let fuel = setting("LUA_FUEL", 1 << 40);
    let memory = setting("LUA_MEMORY", Limits::DEFAULT.memory as u64) as usize;
    let limits = Limits {
        fuel_per_call: fuel,
        fuel_per_tick: fuel,
        memory,
        ..Limits::DEFAULT
    };
    let mut lua = Lua::new(limits, 0).expect("an interpreter");
    lua.set_print(Box::new(|line| {
        std::io::stdout().write_all(line).expect("write to stdout");
    }));
    lua.set_loader(
        FolderLoader::new(&folder, limits.module_size as u64).expect("the script's folder"),
    );
    let chunk = format!(
        "{}",
        path.file_name()
            .map(|n| n.to_string_lossy())
            .unwrap_or_default()
    );
    if let Err(error) = lua.exec(&chunk, &source) {
        std::io::stdout().flush().expect("flush stdout");
        eprintln!("lua: {error}");
        std::process::exit(1);
    }
}
