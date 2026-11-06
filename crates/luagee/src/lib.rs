//! A sandboxed Lua 5.1 interpreter in safe Rust, for running scripts nobody vouches for under hard limits.
//!
//! The language is Lua 5.1 with the extensions LuaJIT 2.1 adds to it (`goto`, the `bit` library, LuaJIT's number
//! syntax), keeping 5.1's function environments (`setfenv`, `getfenv`, `module`). The host gives scripts whatever
//! API they call, through host functions and userdata.
//!
//! # The sandbox
//!
//! A script is untrusted input, as a network peer is. Whatever it does, the host keeps running:
//! - no `unsafe` code in this crate, and no dependencies: what runs a script is this crate and the standard library;
//! - every call runs on fuel counted in VM instructions, and every library function pays fuel for the work it does
//!   (bytes copied, pattern steps, values moved, comparisons, allocations, the collector's walks), so no input makes
//!   it work for free; an interpreter's memory is capped by its own count, not the allocator's;
//! - running out of fuel or memory, or loading too much source, ends the call with [`Error::Limit`]: no `pcall`
//!   catches it, so no script retries its way past a bound. The other bounds of [`Limits`] raise Lua's own errors,
//!   which a script may catch (every retry pays fuel): "stack overflow", "string length overflow", "pattern too
//!   complex", "chunk has too many syntax levels";
//! - Lua calls are frames on the interpreter's heap; Rust recursion (the parser, the compiler, a native function
//!   calling Lua, the pattern matcher) is bounded, so an interpreter needs a thread with a few megabytes of stack;
//! - table keys hash with a seeded SipHash a script never sees, so no script can make its keys collide;
//! - no IO, no OS, no FFI, no JIT, no bytecode: `load`, `loadstring` and `dofile` take source text only; there is no
//!   `string.dump`, `newproxy`, `io`, `os`, `ffi` or `jit`, and `debug` has only `traceback` and a `getinfo` that tells
//!   a function's source and lines;
//! - `require` and `dofile` name a module, never a path: [`ModuleName`] admits only names that stay inside the script
//!   folder, and the host reads it;
//! - an error inside a script ends that call with an [`Error`]; it never panics the host, and the interpreter runs
//!   the next call.
//!
//! # The dialect
//!
//! LuaJIT 2.1's, checked against LuaJIT on a conformance corpus (`tests/conformance`) and by random
//! programs (`tools/lua_diff`): its numbers (literals and `tonumber` with `0x` floats, `0b`, `inf`, `nan`; `%.14g`
//! printing), its library extensions (`goto`, `bit`, `string.rep`'s separator, 64-bit `%d`/`%x`, `table.getn`,
//! `foreach`, `maxn`, `table.new`, `table.clear`, `coroutine.isyieldable`), its error messages, and a coroutine yielding through `pcall`. Where
//! it differs: a coroutine cannot yield inside a metamethod or a library callback (a `table.sort` comparator, a
//! `gsub` function), as in Lua 5.1; `__mode` and `__gc` have no effect (weak tables hold their entries); and the
//! syntax that only recent LuaJIT builds accept (`//`, `&`, `<<`, `!=`, `+=`) does not parse.
//!
//! # Determinism
//!
//! Not promised yet, but designed in, so that turning it on changes no data structure: a table's iteration order
//! follows its history, never an address; `tostring` of a table or function prints an id; collection runs at points
//! the interpreter's own accounting decides; numbers follow the numeric rule of [`number`], and `math.random` draws
//! from a stream the host seeds.
//!
//! # Use
//!
//! [`Lua::new`] makes an interpreter with the bounded standard library open; [`Lua::load`] compiles source,
//! [`Lua::call`] runs a function under the call's and the tick's fuel. `print` writes where [`Lua::set_print`] says,
//! and `require`/`dofile` read through the host's [`Loader`] ([`Lua::set_loader`]).

#![forbid(unsafe_code)]

mod ast;
mod compiler;
mod error;
mod heap;
mod lexer;
mod limits;
mod loader;
mod lua;
mod module_name;
pub mod number;
mod parser;
mod proto;
mod stdlib;
mod table;
mod value;
mod vm;

pub use error::{Error, Limit};

/// A script's bytes as an error message shows them: at most 60 of them, so that no message copies a huge string.
pub(crate) fn shown(bytes: &[u8]) -> String {
    const SHOWN: usize = 60;
    if bytes.len() <= SHOWN {
        return String::from_utf8_lossy(bytes).into_owned();
    }
    format!("{}...", String::from_utf8_lossy(&bytes[..SHOWN]))
}
pub use heap::HostFn;
pub use limits::Limits;
#[cfg(feature = "fs")]
pub use loader::FolderLoader;
pub use loader::Loader;
pub use lua::{HostFunction, Keys, Lua};
pub use module_name::{ModuleName, ModuleNameError};
pub use value::{FuncId, StrId, TableId, ThreadId, UserdataId, Value};
pub use vm::{Args, GcStress, LuaError, PrintSink, Traceback, Vm};
