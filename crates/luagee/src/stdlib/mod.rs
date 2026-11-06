//! The standard library, bounded: every function that loops, copies or allocates in proportion to its input
//! spends fuel for it, and none reaches outside the interpreter.

mod base;
mod bit;
mod coroutine;
mod debug;
mod math;
mod pattern;
mod string;
mod table;

use crate::heap::{Function, NativeFn};
use crate::value::{StrId, TableId, Value};
use crate::vm::{Args, LuaError, Vm};

pub(crate) use base::compile as compile_source;

pub(crate) type Native = fn(&mut Vm, Args) -> Result<usize, LuaError>;

/// The stream `math.random` draws from: xoshiro256** seeded by SplitMix64 from the host's seed.
#[derive(Clone, Debug)]
pub(crate) struct Random {
    state: [u64; 4],
}

impl Random {
    pub(crate) fn new(seed: u64) -> Random {
        let mut random = Random { state: [0; 4] };
        random.seed(seed);
        random
    }

    pub(crate) fn seed(&mut self, seed: u64) {
        let mut x = seed;
        for slot in &mut self.state {
            x = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
            let mut z = x;
            z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
            *slot = z ^ (z >> 31);
        }
    }

    pub(crate) fn next(&mut self) -> u64 {
        let s = &mut self.state;
        let result = s[1].wrapping_mul(5).rotate_left(7).wrapping_mul(9);
        let t = s[1] << 17;
        s[2] ^= s[0];
        s[3] ^= s[1];
        s[1] ^= s[2];
        s[0] ^= s[3];
        s[2] ^= t;
        s[3] = s[3].rotate_left(45);
        result
    }
}

/// Opens the bounded standard library into the globals.
pub(crate) fn open(vm: &mut Vm) -> Result<(), LuaError> {
    base::open(vm)?;
    string::open(vm)?;
    table::open(vm)?;
    math::open(vm)?;
    bit::open(vm)?;
    coroutine::open(vm)?;
    debug::open(vm)?;
    Ok(())
}

impl Vm {
    pub(crate) fn native(
        &mut self,
        name: &'static str,
        f: Native,
        upvalues: Vec<Value>,
    ) -> Result<Value, LuaError> {
        let function = Function::Native {
            function: NativeFn::Builtin(f),
            upvalues: upvalues.into(),
            name,
        };
        Ok(Value::Function(self.heap.new_function(function)?))
    }

    /// Sets `table[name]` to a native function.
    pub(crate) fn register(
        &mut self,
        table: TableId,
        name: &'static str,
        f: Native,
    ) -> Result<(), LuaError> {
        let function = self.native(name, f, Vec::new())?;
        self.set_field(table, name, function)
    }

    pub(crate) fn set_field(
        &mut self,
        table: TableId,
        name: &str,
        value: Value,
    ) -> Result<(), LuaError> {
        let key = Value::String(self.heap.intern(name.as_bytes())?);
        self.raw_set_checked(table, key, value)
    }

    pub(crate) fn get_field(&mut self, table: TableId, name: &str) -> Result<Value, LuaError> {
        let key = Value::String(self.heap.intern(name.as_bytes())?);
        Ok(self.heap.raw_get(table, key))
    }

    /// A library table in the globals.
    pub(crate) fn library(
        &mut self,
        name: &str,
        functions: &[(&'static str, Native)],
    ) -> Result<TableId, LuaError> {
        let table = self.heap.new_table(0, functions.len())?;
        for &(field, f) in functions {
            self.register(table, field, f)?;
        }
        self.set_field(self.globals, name, Value::Table(table))?;
        self.set_field(self.loaded, name, Value::Table(table))?;
        Ok(table)
    }

    /// The name a native error message gives the running function.
    /// "bad argument #i to 'name' (message)", the function named as its caller named it. Called as a method, the
    /// object is argument 0: a bad one is "calling 'name' on bad self".
    #[must_use = "return or propagate the argument error"]
    pub fn arg_error(&mut self, args: Args, i: usize, message: &str) -> LuaError {
        if !self.args_are_live(args) {
            return self.error("expired callback arguments");
        }
        let (kind, name) = match self.callee() {
            Some((kind, name)) => (kind, crate::shown(&name)),
            None => ("", "?".to_owned()),
        };
        if kind == "method" {
            if i == 0 {
                return self.error(format!("calling '{name}' on bad self ({message})"));
            }
            return self.error(format!("bad argument #{i} to '{name}' ({message})"));
        }
        self.error(format!(
            "bad argument #{} to '{name}' ({message})",
            i.saturating_add(1)
        ))
    }

    fn type_name_of_arg(&self, args: Args, i: usize) -> &'static str {
        if self.args_are_live(args) && i < args.count {
            self.arg(args, i).type_name()
        } else {
            "no value"
        }
    }

    pub(crate) fn arg_type_error(&mut self, args: Args, i: usize, expected: &str) -> LuaError {
        let got = self.type_name_of_arg(args, i);
        self.arg_error(args, i, &format!("{expected} expected, got {got}"))
    }

    /// A required argument, including an explicit nil; missing arguments raise a positioned Lua error.
    pub fn check_any(&mut self, args: Args, i: usize) -> Result<Value, LuaError> {
        self.check_args(args)?;
        if i >= args.count {
            return Err(self.arg_error(args, i, "value expected"));
        }
        Ok(self.arg(args, i))
    }

    /// A required number, accepting numeric strings as Lua does; NaN and infinity are numbers too.
    pub fn check_number(&mut self, args: Args, i: usize) -> Result<f64, LuaError> {
        self.check_args(args)?;
        match self.coerce_number(self.arg(args, i)) {
            Some(n) => Ok(n),
            None => Err(self.arg_type_error(args, i, "number")),
        }
    }

    /// An integer argument, truncated toward zero as C's casts do.
    pub fn check_integer(&mut self, args: Args, i: usize) -> Result<i64, LuaError> {
        Ok(crate::number::truncate(self.check_number(args, i)?))
    }

    /// An integer argument, or the default for missing/nil; indexes are zero-based.
    pub fn opt_integer(&mut self, args: Args, i: usize, default: i64) -> Result<i64, LuaError> {
        self.check_args(args)?;
        match self.arg(args, i) {
            Value::Nil => Ok(default),
            _ => self.check_integer(args, i),
        }
    }

    /// A string argument; a number becomes its string, as Lua's libraries convert it.
    pub fn check_string(&mut self, args: Args, i: usize) -> Result<StrId, LuaError> {
        self.check_args(args)?;
        match self.arg(args, i) {
            Value::String(s) => Ok(s),
            Value::Number(n) => {
                let s = self.number_string(n)?;
                self.stack[args.base + i] = Value::String(s);
                Ok(s)
            }
            _ => Err(self.arg_type_error(args, i, "string")),
        }
    }

    /// A string argument (coercing numbers), or None for missing/nil.
    pub fn opt_string(&mut self, args: Args, i: usize) -> Result<Option<StrId>, LuaError> {
        self.check_args(args)?;
        match self.arg(args, i) {
            Value::Nil => Ok(None),
            _ => self.check_string(args, i).map(Some),
        }
    }

    /// A required table argument; a value of any other type raises a positioned Lua error.
    pub fn check_table(&mut self, args: Args, i: usize) -> Result<TableId, LuaError> {
        self.check_args(args)?;
        match self.arg(args, i) {
            Value::Table(t) => Ok(t),
            _ => Err(self.arg_type_error(args, i, "table")),
        }
    }

    /// Pushes results and returns their count.
    pub fn results(&mut self, values: &[Value]) -> Result<usize, LuaError> {
        self.charge_fuel((values.len() / crate::vm::VALUES_PER_FUEL) as u64)?;
        for &value in values {
            self.validate(value)?;
        }
        for &value in values {
            self.push(value)?;
        }
        Ok(values.len())
    }

    pub(crate) fn push_string(&mut self, bytes: &[u8]) -> Result<usize, LuaError> {
        let value = self.new_string(bytes)?;
        self.results(&[value])
    }

    /// `tostring`: what `__tostring` returns (whatever it is, as LuaJIT's `tostring` gives it), else the value's text
    /// (tables and functions by a stable id, never an address).
    pub(crate) fn display_value(&mut self, value: Value) -> Result<Value, LuaError> {
        let handler = self.metamethod(value, self.names.tostring);
        if handler != Value::Nil {
            return self.call_meta(handler, &[value]);
        }
        let text = match value {
            Value::Nil => b"nil".to_vec(),
            Value::Boolean(true) => b"true".to_vec(),
            Value::Boolean(false) => b"false".to_vec(),
            Value::Number(n) => crate::number::format(n).into_bytes(),
            Value::String(_) => return Ok(value),
            Value::Table(t) => format!("table: 0x{:08x}", t.0 as u32).into_bytes(),
            Value::Function(f) => match self.heap.functions.get(f.0) {
                Function::Native { .. } => {
                    format!("function: builtin: 0x{:08x}", f.0 as u32).into_bytes()
                }
                Function::Lua { .. } => format!("function: 0x{:08x}", f.0 as u32).into_bytes(),
            },
            Value::Userdata(u) => format!("userdata: 0x{:08x}", u.0 as u32).into_bytes(),
            Value::Thread(t) => format!("thread: 0x{:08x}", t.0 as u32).into_bytes(),
            Value::Cell(_) => unreachable!("scripts never hold a cell"),
        };
        self.new_string(&text)
    }
}
