//! The bounds on what a script may take.

/// The bounds on what a script may take. Running out of fuel or memory, or loading too much source, ends the call
/// with [`Error::Limit`](crate::Error::Limit), which no `pcall` catches; the other bounds raise Lua's own errors,
/// which scripts may catch (every retry pays fuel).
///
/// The defaults suit a host that calls scripts many times a second: an interpreter runs some 50 to 250 million fuel
/// units a second (allocation-heavy code is the slow end), so a whole tick's budget takes at most 10 to 40 ms; a few
/// hundred source files of several megabytes in all load well inside the source bounds.
///
/// More bounds may be added, so the struct cannot be written out field by field outside this crate: start from
/// [`Limits::DEFAULT`] and change what differs, with the `with_*` methods (they are `const`, so a limit can be a
/// constant) or by assigning the fields.
///
/// ```
/// use luagee::Limits;
///
/// const SMALL: Limits = Limits::DEFAULT.with_memory(8 << 20).with_function_upvalues(120);
/// let mut limits = SMALL;
/// limits.call_depth = 200;
/// assert_eq!(limits.memory, 8 << 20);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
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
    /// How deeply source may nest (parentheses, blocks, table constructors), which bounds the parser's and the
    /// compiler's recursion ("chunk has too many syntax levels"): up to 2 KiB of native stack a level.
    pub syntax_depth: u32,
    /// How deeply a string pattern's matcher may recurse (optional items, expansions, captures): "pattern too
    /// complex". Some 0.3 KiB of native stack a level.
    pub pattern_depth: u32,
    /// The upvalues one function may capture ("function at line N has more than M upvalues"): 60 as in stock Lua 5.1
    /// and LuaJIT, a script's nested functions counting as they do there. A byte because that is how wide an upvalue's
    /// index is in the bytecode, so no value can name more than the representation holds (255 is stock Lua's own
    /// hard maximum). Raise it to run scripts written for a runtime built with a larger limit; each closure then
    /// holds up to this many cells, counted as memory and as fuel like any other closure.
    pub function_upvalues: u8,
    /// The bytes of one module's source.
    pub module_size: usize,
    /// The bytes of all the source one interpreter loads.
    pub scripts_size: usize,
    /// The modules one interpreter may load.
    pub module_count: u32,
}

impl Limits {
    /// These limits with [`fuel_per_call`](Limits::fuel_per_call) set.
    #[must_use]
    pub const fn with_fuel_per_call(mut self, fuel_per_call: u64) -> Limits {
        self.fuel_per_call = fuel_per_call;
        self
    }

    /// These limits with [`fuel_per_tick`](Limits::fuel_per_tick) set.
    #[must_use]
    pub const fn with_fuel_per_tick(mut self, fuel_per_tick: u64) -> Limits {
        self.fuel_per_tick = fuel_per_tick;
        self
    }

    /// These limits with [`memory`](Limits::memory) set.
    #[must_use]
    pub const fn with_memory(mut self, memory: usize) -> Limits {
        self.memory = memory;
        self
    }

    /// These limits with [`string_length`](Limits::string_length) set.
    #[must_use]
    pub const fn with_string_length(mut self, string_length: usize) -> Limits {
        self.string_length = string_length;
        self
    }

    /// These limits with [`call_depth`](Limits::call_depth) set.
    #[must_use]
    pub const fn with_call_depth(mut self, call_depth: u32) -> Limits {
        self.call_depth = call_depth;
        self
    }

    /// These limits with [`syntax_depth`](Limits::syntax_depth) set.
    #[must_use]
    pub const fn with_syntax_depth(mut self, syntax_depth: u32) -> Limits {
        self.syntax_depth = syntax_depth;
        self
    }

    /// These limits with [`pattern_depth`](Limits::pattern_depth) set.
    #[must_use]
    pub const fn with_pattern_depth(mut self, pattern_depth: u32) -> Limits {
        self.pattern_depth = pattern_depth;
        self
    }

    /// These limits with [`function_upvalues`](Limits::function_upvalues) set.
    #[must_use]
    pub const fn with_function_upvalues(mut self, function_upvalues: u8) -> Limits {
        self.function_upvalues = function_upvalues;
        self
    }

    /// These limits with [`module_size`](Limits::module_size) set.
    #[must_use]
    pub const fn with_module_size(mut self, module_size: usize) -> Limits {
        self.module_size = module_size;
        self
    }

    /// These limits with [`scripts_size`](Limits::scripts_size) set.
    #[must_use]
    pub const fn with_scripts_size(mut self, scripts_size: usize) -> Limits {
        self.scripts_size = scripts_size;
        self
    }

    /// These limits with [`module_count`](Limits::module_count) set.
    #[must_use]
    pub const fn with_module_count(mut self, module_count: u32) -> Limits {
        self.module_count = module_count;
        self
    }
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
        function_upvalues: 60,
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
