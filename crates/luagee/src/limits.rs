//! The bounds on what a script may take.

/// The bounds on what a script may take. Running out of fuel or memory, or loading too much source, ends the call
/// with [`Error::Limit`](crate::Error::Limit), which no `pcall` catches; the other bounds raise Lua's own errors,
/// which scripts may catch (every retry pays fuel).
///
/// The defaults suit a host that calls scripts many times a second: an interpreter runs some 50 to 250 million fuel
/// units a second (allocation-heavy code is the slow end), so a whole tick's budget takes at most 10 to 40 ms; a few
/// hundred source files of several megabytes in all load well inside the source bounds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// The VM instructions one call from the host may run. Pattern matching charges its steps here too.
    pub fuel_per_call: u64,
    /// The VM instructions all calls of one host tick may run, so that many small calls cannot add up to a stall.
    pub fuel_per_tick: u64,
    /// The bytes one interpreter may hold, as its own accounting counts them (a value, a table slot, a string's bytes),
    /// so that the cap does not depend on the allocator or the platform.
    pub memory: usize,
    /// The bytes of the longest string a script may build ("string length overflow").
    pub string_length: usize,
    /// The Lua calls that may be running at once ("stack overflow"). Calls are frames on the interpreter's heap, never on
    /// the Rust stack.
    pub call_depth: u32,
    /// How deeply source may nest (parentheses, blocks, table constructors), which bounds the parser's recursion
    /// ("chunk has too many syntax levels").
    pub syntax_depth: u32,
    /// How deeply a string pattern's matcher may recurse (optional items, expansions, captures): "pattern too
    /// complex".
    pub pattern_depth: u32,
    /// The bytes of one module's source.
    pub module_size: usize,
    /// The bytes of all the source one interpreter loads.
    pub scripts_size: usize,
    /// The modules one interpreter may load.
    pub module_count: u32,
}

impl Limits {
    /// The defaults.
    pub const DEFAULT: Limits = Limits {
        fuel_per_call: 500_000,
        fuel_per_tick: 2_000_000,
        memory: 256 << 20,
        string_length: 4 << 20,
        call_depth: 10_000,
        syntax_depth: 200,
        pattern_depth: 200,
        module_size: 1 << 20,
        scripts_size: 32 << 20,
        module_count: 2048,
    };
}

impl Default for Limits {
    fn default() -> Self {
        Limits::DEFAULT
    }
}

// A call can use its whole budget within a tick, a string fits in memory, and a module fits in the scripts.
const _: () = assert!(Limits::DEFAULT.fuel_per_call <= Limits::DEFAULT.fuel_per_tick);
const _: () = assert!(Limits::DEFAULT.string_length < Limits::DEFAULT.memory);
const _: () = assert!(Limits::DEFAULT.module_size <= Limits::DEFAULT.scripts_size);
