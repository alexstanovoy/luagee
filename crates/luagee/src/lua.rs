//! The host's handle on an interpreter.

use std::rc::Rc;

use crate::heap::{Function, NativeFn};
use crate::parser::ParseError;
use crate::value::{StrId, TableId, Value};
use crate::vm::{LuaError, PrintSink, Thrown, Vm};
use crate::{Error, Limit, Limits};

/// A host function: it reads its arguments with [`Vm`]'s accessors and pushes its results, returning their count.
pub type HostFunction = Rc<crate::heap::HostFn>;

/// What an interpreter's randomness comes from: both chosen by the host, as the library takes nothing from the
/// operating system (it runs the same everywhere, wasm included).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Keys {
    /// The key of the table hash. It must be secret to the scripts and different for every interpreter (draw it from
    /// the host's entropy): a script that learned it could choose keys that collide. Iteration order never depends
    /// on it, so it changes no script's output.
    pub hash: [u64; 2],
    /// The seed of `math.random`'s stream: the same seed and calls give the same numbers.
    pub random: u64,
}

/// A sandboxed interpreter with the standard library open. Interpreters share nothing: a host isolates scripts by
/// giving each group its own.
pub struct Lua {
    vm: Vm,
}

/// Compiles source into a function in the thread's globals: Ok(Err(message)) when it does not compile.
pub(crate) fn compile_chunk(
    vm: &mut Vm,
    source: &[u8],
    name: StrId,
    chunk: &str,
) -> Result<Result<Value, String>, LuaError> {
    let budget = vm.heap.cap.saturating_sub(vm.heap.memory);
    let block = match crate::parser::parse(
        source,
        vm.limits.syntax_depth,
        vm.limits.string_length,
        budget,
    ) {
        Ok(block) => block,
        Err(ParseError::Syntax(e)) => return Ok(Err(format!("{chunk}:{}: {}", e.line, e.message))),
        Err(ParseError::Limit(limit)) => return Err(LuaError::Limit(limit)),
    };
    let proto = match crate::compiler::compile(
        &block,
        chunk.into(),
        name,
        &mut vm.heap,
        vm.limits.syntax_depth,
    ) {
        Ok(proto) => proto,
        Err(ParseError::Syntax(e)) => return Ok(Err(format!("{chunk}:{}: {}", e.line, e.message))),
        Err(ParseError::Limit(limit)) => return Err(LuaError::Limit(limit)),
    };
    let function = Function::Lua {
        proto,
        upvalues: Rc::from(Vec::new()),
        env: vm.globals,
    };
    Ok(Ok(Value::Function(vm.heap.new_function(function)?)))
}

impl Lua {
    /// An interpreter with these limits and keys.
    pub fn new(limits: Limits, keys: Keys) -> Result<Lua, Error> {
        let mut vm = Vm::new(limits, keys).map_err(|e| to_error(&mut None, e))?;
        crate::stdlib::open(&mut vm).map_err(|e| to_error(&mut None, e))?;
        Ok(Lua { vm })
    }

    /// Where `print` writes; without one it writes nothing.
    pub fn set_print(&mut self, sink: PrintSink) {
        self.vm.print = Some(sink);
    }

    /// What `require` and `dofile` read modules from; without one, no module is found.
    pub fn set_loader(&mut self, loader: impl crate::Loader + 'static) {
        self.vm.loader = Some(Box::new(loader));
    }

    /// Starts a host tick: the fuel all calls of the tick may spend is full again.
    pub fn begin_tick(&mut self) {
        self.vm.tick_fuel = self.vm.limits.fuel_per_tick;
    }

    /// The fuel the current tick has left; what the tick's calls spent is `fuel_per_tick` minus this.
    #[must_use]
    pub fn tick_fuel_left(&self) -> u64 {
        self.vm.tick_fuel
    }

    /// Compiles source into a function (not run yet). The chunk name is what error positions print.
    pub fn load(&mut self, chunk: &str, source: &[u8]) -> Result<Value, Error> {
        let vm = &mut self.vm;
        // The host's own loading is not a script's work: it spends no script fuel (the source limits still hold).
        let fuel = std::mem::replace(&mut vm.fuel, u64::MAX);
        // As a file is in Lua, the chunk's source is "@" and its name.
        let compiled = vm
            .heap
            .intern(format!("@{chunk}").as_bytes())
            .and_then(|name| crate::stdlib::compile_source(vm, source, name, chunk));
        vm.fuel = fuel;
        let compiled = compiled.map_err(|e| to_error(&mut Some(vm), e))?;
        compiled.map_err(|message: String| Error::Syntax {
            chunk: chunk.to_owned(),
            line: line_of(&message),
            message,
        })
    }

    /// Calls a function with arguments under the call's and the tick's fuel; its results.
    pub fn call(&mut self, function: Value, args: &[Value]) -> Result<Vec<Value>, Error> {
        let vm = &mut self.vm;
        let budget = vm.limits.fuel_per_call.min(vm.tick_fuel);
        vm.fuel = budget;
        vm.fuel_limit = if vm.limits.fuel_per_call <= vm.tick_fuel {
            Limit::FuelPerCall
        } else {
            Limit::FuelPerTick
        };
        let slot = vm.stack.len();
        let result = (|| {
            vm.push(function)?;
            for &arg in args {
                vm.push(arg)?;
            }
            let count = vm.call_value(slot, args.len())?;
            Ok(vm.stack[slot..slot + count].to_vec())
        })();
        vm.tick_fuel -= budget - vm.fuel;
        vm.stack.truncate(slot);
        vm.frames.clear();
        vm.native_depth = 0;
        vm.yielding = false;
        result.map_err(|e| {
            let e = vm.locate(e, None);
            to_error(&mut Some(vm), e)
        })
    }

    /// Loads and runs a chunk.
    pub fn exec(&mut self, chunk: &str, source: &[u8]) -> Result<Vec<Value>, Error> {
        let function = self.load(chunk, source)?;
        self.call(function, &[])
    }

    /// Keeps a value alive across calls until [`unpin`](Self::unpin).
    pub fn pin(&mut self, value: Value) {
        self.vm.pinned.push(value);
    }

    /// Lets the collector take a pinned value again.
    pub fn unpin(&mut self, value: Value) {
        if let Some(i) = self.vm.pinned.iter().rposition(|v| *v == value) {
            self.vm.pinned.swap_remove(i);
        }
    }

    /// A string value.
    pub fn string(&mut self, bytes: &[u8]) -> Result<Value, Error> {
        let vm = &mut self.vm;
        vm.new_string(bytes).map_err(|e| to_error(&mut Some(vm), e))
    }

    /// A string value's bytes.
    #[must_use]
    pub fn bytes(&self, value: Value) -> Option<&[u8]> {
        match value {
            Value::String(s) => Some(self.vm.heap.string(s)),
            _ => None,
        }
    }

    /// The globals table.
    #[must_use]
    pub fn globals(&self) -> TableId {
        self.vm.globals
    }

    /// `globals[name]`, without metamethods.
    pub fn global(&mut self, name: &str) -> Result<Value, Error> {
        let vm = &mut self.vm;
        vm.get_field(vm.globals, name)
            .map_err(|e| to_error(&mut Some(vm), e))
    }

    /// `globals[name] = value`, without metamethods.
    pub fn set_global(&mut self, name: &str, value: Value) -> Result<(), Error> {
        let vm = &mut self.vm;
        vm.set_field(vm.globals, name, value)
            .map_err(|e| to_error(&mut Some(vm), e))
    }

    /// A function value calling the host.
    pub fn function(&mut self, name: &'static str, f: HostFunction) -> Result<Value, Error> {
        let vm = &mut self.vm;
        let function = Function::Native {
            function: NativeFn::Host(f),
            upvalues: Box::new([]),
            name,
        };
        let id = vm
            .heap
            .new_function(function)
            .map_err(|e| to_error(&mut Some(vm), e))?;
        Ok(Value::Function(id))
    }

    /// A userdata value carrying host data, with a metatable (its methods and operators) or none.
    pub fn userdata(
        &mut self,
        data: Box<dyn std::any::Any>,
        metatable: Option<TableId>,
    ) -> Result<Value, Error> {
        let vm = &mut self.vm;
        let id = vm
            .heap
            .new_userdata(data, metatable)
            .map_err(|e| to_error(&mut Some(vm), e))?;
        Ok(Value::Userdata(id))
    }

    /// The host data of a userdata value, if it is one of type `T`.
    #[must_use]
    pub fn userdata_ref<T: 'static>(&self, value: Value) -> Option<&T> {
        match value {
            Value::Userdata(u) => self.vm.heap.userdata.get(u.0).data.downcast_ref(),
            _ => None,
        }
    }

    /// The bytes the interpreter holds, as it counts them.
    #[must_use]
    pub fn memory(&self) -> usize {
        self.vm.heap.memory
    }

    /// Runs a whole collection now. It fails only when what is alive is over the memory cap.
    pub fn collect(&mut self) -> Result<(), Error> {
        let vm = &mut self.vm;
        vm.fuel = u64::MAX;
        let result = vm.collect();
        vm.fuel = 0;
        result.map_err(|e| to_error(&mut Some(vm), e))
    }

    /// A test mode of the collector: collect far more often than needed, so that a value the collector fails to see
    /// shows up at once. For tests and fuzzing; it makes everything much slower.
    #[doc(hidden)]
    pub fn set_gc_stress(&mut self, stress: crate::vm::GcStress) {
        self.vm.gc_stress = stress;
        self.vm.gc_threshold = self.vm.next_threshold(self.vm.heap.memory);
    }

    /// The interpreter, for host functions' argument accessors.
    pub fn vm(&mut self) -> &mut Vm {
        &mut self.vm
    }
}

fn line_of(message: &str) -> u32 {
    message
        .split(':')
        .nth(1)
        .and_then(|l| l.parse().ok())
        .unwrap_or(0)
}

/// The host's view of an error: the message as text.
fn to_error(vm: &mut Option<&mut Vm>, error: LuaError) -> Error {
    match error {
        LuaError::Limit(limit) => Error::Limit(limit),
        LuaError::Runtime(thrown) => {
            let Thrown {
                value, traceback, ..
            } = *thrown;
            let message = match (vm, value) {
                (Some(vm), Value::String(s)) => text(vm, s),
                (Some(vm), Value::Number(n)) => {
                    let _ = vm;
                    crate::number::format(n)
                }
                (_, Value::Nil) => "nil".to_owned(),
                (_, other) => format!("(error object is a {} value)", other.type_name()),
            };
            Error::Runtime {
                message,
                traceback: traceback.lines(),
            }
        }
    }
}

fn text(vm: &Vm, s: StrId) -> String {
    String::from_utf8_lossy(vm.heap.string(s)).into_owned()
}
