//! What ends a call into a script early.

use std::fmt;

use crate::LimitsError;

pub(crate) const RETIRED_MESSAGE: &str = "interpreter retired after a host callback panic";

/// What ended a call into a script: the host gets this, never a panic, and the interpreter stays usable.
#[derive(Clone, Debug, PartialEq, Eq)]
#[must_use]
pub enum Error {
    /// The source does not parse.
    Syntax {
        /// The chunk's name, as the host gave it (a module name, never a host path).
        chunk: String,
        /// The line, from 1.
        line: u32,
        /// What is wrong, in Lua's words.
        message: String,
    },
    /// The script raised an error, or broke a rule of the language, and nothing caught it.
    Runtime {
        /// The error value as text, as `tostring` gives it.
        message: String,
        /// The Lua calls that were running, innermost first, as `chunk:line` (no addresses).
        traceback: Vec<String>,
    },
    /// The script ran out of fuel or memory, or loaded too much source: `pcall` cannot catch this, so a script cannot
    /// retry its way past a bound. (The other bounds raise Lua's own catchable errors: "stack overflow", "string
    /// length overflow", "pattern too complex", "chunk has too many syntax levels".)
    Limit(Limit),
    /// [`Lua::new`](crate::Lua::new) was given limits that cannot hold an interpreter.
    InvalidLimits(LimitsError),
}

/// Which uncatchable bound of the [`Limits`](crate::Limits) a script crossed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Limit {
    /// [`Limits::fuel_per_call`](crate::Limits::fuel_per_call).
    FuelPerCall,
    /// [`Limits::fuel_per_tick`](crate::Limits::fuel_per_tick).
    FuelPerTick,
    /// [`Limits::memory`](crate::Limits::memory).
    Memory,
    /// [`Limits::module_size`](crate::Limits::module_size).
    ModuleSize,
    /// [`Limits::scripts_size`](crate::Limits::scripts_size).
    ScriptsSize,
    /// [`Limits::module_count`](crate::Limits::module_count).
    ModuleCount,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Syntax {
                chunk,
                line,
                message,
            } => write!(f, "{chunk}:{line}: {message}"),
            Error::Runtime { message, traceback } => {
                write!(f, "{message}")?;
                for frame in traceback {
                    write!(f, "\n\tin {frame}")?;
                }
                Ok(())
            }
            Error::Limit(limit) => write!(f, "{limit}"),
            Error::InvalidLimits(error) => write!(f, "invalid limits: {error}"),
        }
    }
}

impl fmt::Display for Limit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Limit::FuelPerCall => "the call ran out of fuel: it ran more instructions than one call may",
            Limit::FuelPerTick => "the tick ran out of fuel: the calls of this tick ran more instructions than a tick may",
            Limit::Memory => "the interpreter ran out of memory: the script holds more than its memory cap",
            Limit::ModuleSize => "a module's source is larger than a module may be",
            Limit::ScriptsSize => "the modules loaded add up to more source than a script may load",
            Limit::ModuleCount => "more modules were loaded than a script may load",
        })
    }
}

impl std::error::Error for Error {}
