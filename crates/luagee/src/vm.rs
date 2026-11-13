//! The interpreter: the dispatch loop, calls, metamethods, errors, coroutines and the collector.
//!
//! Lua calls are frames on the interpreter's own stack, never Rust recursion. A native function (a library
//! function or a metamethod run from inside an instruction) that calls back into Lua nests the loop once, and that
//! nesting is bounded; a coroutine cannot yield across it, as in Lua 5.1.

use std::rc::Rc;

use crate::Limit;
use crate::Limits;
use crate::heap::{Arena, Function, Heap, NativeFn, Partial, Phase};
use crate::number;
use crate::proto::{ArithOp, Capture, CompareOp, Op, Proto, Rk};
use crate::table::Table;
use crate::value::{CellId, FuncId, HANDLE_BYTES, StrId, TableId, ThreadId, VALUE_BYTES, Value};

/// How deeply native functions may nest Lua calls ("C stack overflow" past it), as Lua 5.1's C calls do.
/// Mixed nesting, parsing and pattern matching are checked on a 1 MiB native stack, and under wasmtime with a
/// 448 KiB engine stack and a 512 KiB module stack (`tests/hostile/deepest_native_nesting_*.lua`). Frame sizes
/// depend on the toolchain and engine instrumentation; embedders must budget their own callback frames too.
pub(crate) const MAX_NATIVE_DEPTH: u32 = 200;
/// How many frames a traceback keeps.
const TRACEBACK_FRAMES: usize = 16;
/// The slots of a coroutine's stack, counted as this many bytes each.
const SLOT_BYTES: usize = VALUE_BYTES;
/// The bytes a frame is counted as.
const FRAME_BYTES: usize = 192;
/// The bytes a coroutine is counted as, beyond its stack.
pub(crate) const THREAD_BYTES: usize = 256;
const _: () = assert!(crate::heap::slot_size::<ThreadState>() <= THREAD_BYTES);
const _: () = assert!(std::mem::size_of::<Frame>() <= FRAME_BYTES);

/// Why a call ended early, inside the interpreter: what host functions return to raise an error.
#[derive(Clone, Debug)]
#[must_use]
pub enum LuaError {
    /// A Lua error, catchable by `pcall`. Boxed: every `Result` the interpreter returns carries an error's size, and
    /// the dispatch loop, which native calls back into Lua nest, holds dozens of them; a small error keeps that
    /// nesting cheap on a small native stack (wasm's).
    Runtime(Box<Thrown>),
    /// A limit was crossed: nothing in the script can catch it.
    Limit(Limit),
}

/// A Lua error on its way to the `pcall` that catches it, or to the host.
#[derive(Clone, Debug)]
pub struct Thrown {
    /// The error value.
    pub value: Value,
    /// Where the Lua calls stood when it was raised.
    pub traceback: Traceback,
    /// Whether the error position is already in the message (or must not be added).
    pub located: bool,
}

impl LuaError {
    /// A Lua error with this value.
    pub fn runtime(value: Value, traceback: Traceback, located: bool) -> LuaError {
        LuaError::Runtime(Box::new(Thrown {
            value,
            traceback,
            located,
        }))
    }
}

// The error stays two words, so that a `Result<Value, LuaError>` stays three.
#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::size_of::<LuaError>() <= 16);

/// Where the Lua calls running at an error stood, innermost first: kept as functions and instruction indexes, and
/// turned into text only when the error reaches the host (most errors are caught and never shown).
#[derive(Clone, Debug, Default)]
pub struct Traceback(Vec<(Rc<Proto>, usize)>);

impl Traceback {
    /// `chunk:line` per frame, innermost first.
    pub(crate) fn lines(&self) -> Vec<String> {
        self.0
            .iter()
            .map(|(proto, pc)| Vm::position(proto, *pc))
            .collect()
    }
}

/// A native function's arguments: `count` values on the stack from `base`.
#[derive(Clone, Copy, Debug)]
pub struct Args {
    pub(crate) owner: u64,
    pub(crate) serial: u64,
    pub(crate) base: usize,
    pub(crate) count: usize,
}

#[derive(Clone, Debug)]
pub(crate) struct Frame {
    pub(crate) function: FuncId,
    pub(crate) proto: Rc<Proto>,
    pub(crate) upvalues: Rc<[CellId]>,
    pub(crate) pc: usize,
    pub(crate) base: usize,
    /// Where the function sat: its results go there.
    pub(crate) slot: usize,
    /// The results the caller wants, -1 for all.
    pub(crate) want: i32,
    pub(crate) varargs: (usize, usize),
    /// Whether returning from this frame ends the loop that runs it (a call from Rust).
    pub(crate) boundary: bool,
    /// Set when this frame is the function a `pcall` or `xpcall` runs: errors below it stop here.
    pub(crate) catch: Option<Catch>,
    /// Set when an instruction of the frame below set this function off as a metamethod handler: though entered
    /// from Rust (a boundary), it is called from Lua code, which is where its errors at level 2 point.
    pub(crate) metamethod: Option<&'static str>,
}

/// A `pcall` or `xpcall` waiting on its function's frame: its handler (nil for `pcall`) and the results its caller
/// wants. The `pcall` itself sat in the slot just below the frame's.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Catch {
    pub(crate) handler: Value,
    pub(crate) want: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ThreadStatus {
    Fresh,
    Suspended,
    Running,
    Normal,
    Dead,
}

#[derive(Debug)]
pub(crate) struct ThreadState {
    pub(crate) stack: Vec<Value>,
    pub(crate) frames: Vec<Frame>,
    pub(crate) top: usize,
    pub(crate) status: ThreadStatus,
    /// Where a suspended coroutine's yield left its values and where the values a resume passes in go.
    pub(crate) pending: Option<Pending>,
    /// The native nesting when this coroutine was last resumed.
    pub(crate) depth: u32,
    /// The bytes of this coroutine's stack already counted against the memory cap (while it runs, `Vm::stack_charged`
    /// holds them).
    pub(crate) charged: usize,
}

impl ThreadState {
    /// The counted bytes of a coroutine out of its slot's storage: its stack and frames.
    fn bytes(&self) -> usize {
        THREAD_BYTES + self.stack.capacity() * SLOT_BYTES + self.frames.capacity() * FRAME_BYTES
    }

    fn new() -> ThreadState {
        ThreadState {
            stack: Vec::new(),
            frames: Vec::new(),
            top: 0,
            status: ThreadStatus::Fresh,
            pending: None,
            depth: 0,
            charged: 0,
        }
    }
}

/// A yield waiting for its resume.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Pending {
    /// Where the yielded values start.
    pub(crate) values: usize,
    /// Where the resume's values go, as the yield call's (or the `pcall` around it) results.
    pub(crate) slot: usize,
    pub(crate) want: i32,
    /// Whether the yield ran inside `pcall(coroutine.yield, ...)`: the resume's values then follow a `true`.
    pub(crate) protected: bool,
}

/// What a `pcall` called from Lua did.
pub(crate) enum Protected {
    /// Pushed its function's frame.
    Frame,
    /// Ran a native function and left its results.
    Done,
    /// Ran `coroutine.yield`: the coroutine suspends.
    Yielded,
}

/// How the running native was called: what its argument errors name it, and where its errors say it was called.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CallSite {
    /// No native runs: Lua code runs its instructions.
    None,
    /// By a call instruction of the running Lua function, of the function in this register.
    Call { register: u8 },
    /// By a tail call of the running Lua function. As in LuaJIT, the function that made the call is gone: the native
    /// reports from that function's caller.
    Tail,
    /// As the handler of a metamethod an instruction of the running Lua function set off.
    Meta { event: &'static str },
    /// By another native.
    Native,
}

impl CallSite {
    /// Whether Lua code called the native (so its errors have a position).
    pub(crate) fn is_from_lua(self) -> bool {
        matches!(
            self,
            CallSite::Call { .. } | CallSite::Tail | CallSite::Meta { .. }
        )
    }
}

/// The collector's test modes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GcStress {
    /// Collect when memory says so.
    Off,
    /// A whole collection at every one of the collector's points.
    Full,
    /// Always an incremental collection under way, a few values traced or slots swept per point: the most interleaving that still lets a collection end.
    Incremental,
}

impl GcStress {
    fn step(self) -> usize {
        if self == GcStress::Incremental {
            8
        } else {
            GC_STEP
        }
    }
}

/// How the loop stopped.
pub(crate) enum Exec {
    Returned,
    Yielded,
}

/// The metamethod names, interned once.
pub(crate) struct Names {
    pub(crate) index: StrId,
    pub(crate) newindex: StrId,
    pub(crate) call: StrId,
    pub(crate) add: StrId,
    pub(crate) sub: StrId,
    pub(crate) mul: StrId,
    pub(crate) div: StrId,
    pub(crate) modulo: StrId,
    pub(crate) pow: StrId,
    pub(crate) unm: StrId,
    pub(crate) concat: StrId,
    pub(crate) len: StrId,
    pub(crate) eq: StrId,
    pub(crate) lt: StrId,
    pub(crate) le: StrId,
    pub(crate) tostring: StrId,
    pub(crate) metatable: StrId,
    pub(crate) name: StrId,
}

/// The host's sink for `print`.
pub type PrintSink = Box<dyn FnMut(&[u8])>;

/// The interpreter's state; host functions get it to read their arguments and push their results.
pub struct Vm {
    pub(crate) owner: u64,
    pub(crate) retired: bool,
    pub(crate) heap: Heap,
    pub(crate) stack: Vec<Value>,
    pub(crate) frames: Vec<Frame>,
    /// The end of the values an open call or `...` left.
    pub(crate) top: usize,
    pub(crate) threads: Arena<ThreadState>,
    pub(crate) current: ThreadId,
    pub(crate) main: ThreadId,
    pub(crate) globals: TableId,
    /// Modules `require` has loaded (`package.loaded`).
    pub(crate) loaded: TableId,
    pub(crate) string_meta: Option<TableId>,
    pub(crate) names: Names,
    /// Values the host keeps across calls.
    pub(crate) pinned: Vec<Value>,
    pub(crate) limits: Limits,
    pub(crate) tick_fuel: u64,
    pub(crate) native_depth: u32,
    pub(crate) host_depth: u32,
    pub(crate) active_serial: u64,
    next_serial: u64,
    /// How the running native was called.
    pub(crate) call_site: CallSite,
    pub(crate) yielding: bool,
    pub(crate) gc_threshold: usize,
    pub(crate) gc_epoch: u64,
    pub(crate) gc_requested: bool,
    /// The arena the sweep is in.
    gc_arena: SweepArena,
    /// The counted bytes the sweep kept so far.
    gc_kept: usize,
    /// Memory as counted when the sweep began.
    gc_memory_at_sweep: usize,
    /// The bytes of compiled code the current collection found alive.
    pub(crate) gc_protos: usize,
    /// A test mode that collects far more often than needed, to find a root or a barrier missing.
    pub(crate) gc_stress: GcStress,
    pub(crate) stack_charged: usize,
    pub(crate) scripts_loaded: usize,
    pub(crate) modules_loaded: u32,
    pub(crate) random: crate::stdlib::Random,
    pub(crate) print: Option<PrintSink>,
    pub(crate) loader: Option<Box<dyn crate::Loader>>,
}

/// Marks an interpreter unusable if Rust unwinding crosses an execution boundary, even when its caller catches it.
struct UnwindScope<'a> {
    vm: &'a mut Vm,
    completed: bool,
}

impl Drop for UnwindScope<'_> {
    fn drop(&mut self) {
        if !self.completed {
            self.vm.retired = true;
            self.vm.heap.fuel.left = 0;
        }
    }
}

/// The fuel an allocation costs beyond its instruction (a string, table or closure: made, counted, collected later).
pub(crate) const ALLOCATION_FUEL: u64 = 4;
/// The values copied for one unit of fuel, where one instruction moves many (calls, returns, `...`, constructors).
pub(crate) const VALUES_PER_FUEL: usize = 8;
/// The objects the collector traces or sweeps for one unit of fuel.
pub(crate) const GC_OBJECTS_PER_FUEL: usize = 4;

#[cfg(test)]
mod tests;

/// The arena a sweep is in, in the order the sweep walks them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SweepArena {
    Strings,
    Tables,
    Functions,
    Cells,
    Userdata,
    Threads,
    /// Every arena swept: the sweep ends.
    Done,
}

impl SweepArena {
    fn next(self) -> SweepArena {
        match self {
            SweepArena::Strings => SweepArena::Tables,
            SweepArena::Tables => SweepArena::Functions,
            SweepArena::Functions => SweepArena::Cells,
            SweepArena::Cells => SweepArena::Userdata,
            SweepArena::Userdata => SweepArena::Threads,
            SweepArena::Threads | SweepArena::Done => SweepArena::Done,
        }
    }
}

/// The most units of one object traced at once.
const TRACE_PIECE: usize = 4096;

/// The work (gray values traced, slots swept) of one step of an incremental collection.
const GC_STEP: usize = 256;
/// The counted bytes of a function object.
fn function_bytes(function: &Function) -> usize {
    crate::heap::FUNCTION_SIZE
        + match function {
            Function::Lua { upvalues, .. } => upvalues.len() * HANDLE_BYTES,
            Function::Native { upvalues, .. } => upvalues.len() * VALUE_BYTES,
        }
}

/// The smallest collection threshold.
const MIN_THRESHOLD: usize = 1 << 20;

/// Saves the running frame's position before an instruction's slow path, which may run a metamethod: what that
/// metamethod's `error(message, 2)` and a native's messages report.
macro_rules! save_pc {
    ($vm:ident, $pc:expr) => {
        $vm.frames.last_mut().expect("a running frame").pc = $pc
    };
}

macro_rules! located {
    ($vm:ident, $result:expr, $pc:expr) => {
        match $result {
            Ok(value) => value,
            Err(error) => return Err($vm.locate(error, Some($pc))),
        }
    };
}

impl Vm {
    /// Allocation never collects: host temporaries stay valid until explicit reentry/collection.
    pub(crate) fn allocate_table(
        &mut self,
        array: usize,
        hash: usize,
    ) -> Result<TableId, LuaError> {
        self.heap.new_table(array, hash)
    }

    pub(crate) fn new(limits: Limits, keys: crate::Keys, owner: u64) -> Result<Vm, LuaError> {
        assert_ne!(owner, 0, "an interpreter has a nonzero owner identity");
        let mut heap = Heap::new(limits.memory, keys.hash, owner);
        let mut intern = |s: &str| heap.intern(s.as_bytes());
        let names = Names {
            index: intern("__index")?,
            newindex: intern("__newindex")?,
            call: intern("__call")?,
            add: intern("__add")?,
            sub: intern("__sub")?,
            mul: intern("__mul")?,
            div: intern("__div")?,
            modulo: intern("__mod")?,
            pow: intern("__pow")?,
            unm: intern("__unm")?,
            concat: intern("__concat")?,
            len: intern("__len")?,
            eq: intern("__eq")?,
            lt: intern("__lt")?,
            le: intern("__le")?,
            tostring: intern("__tostring")?,
            metatable: intern("__metatable")?,
            name: intern("__name")?,
        };
        let globals = heap.new_table(0, 64)?;
        let loaded = heap.new_table(0, 16)?;
        let mut threads = Arena::new(owner);
        let main = ThreadId(threads.insert(ThreadState::new()));
        threads.get_mut(main.0).status = ThreadStatus::Running;
        heap.fuel.left = limits.fuel_per_call;
        Ok(Vm {
            owner,
            retired: false,
            heap,
            stack: Vec::new(),
            frames: Vec::new(),
            top: 0,
            threads,
            current: main,
            main,
            globals,
            loaded,
            string_meta: None,
            names,
            pinned: Vec::new(),
            limits,
            tick_fuel: limits.fuel_per_tick,
            native_depth: 0,
            host_depth: 0,
            active_serial: 0,
            next_serial: 0,
            call_site: CallSite::None,
            yielding: false,
            gc_threshold: MIN_THRESHOLD,
            gc_epoch: 0,
            gc_requested: false,
            gc_arena: SweepArena::Strings,
            gc_kept: 0,
            gc_memory_at_sweep: 0,
            gc_protos: 0,
            gc_stress: GcStress::Off,
            stack_charged: 0,
            scripts_loaded: 0,
            modules_loaded: 0,
            random: crate::stdlib::Random::new(keys.random),
            print: None,
            loader: None,
        })
    }

    // --- errors ----------------------------------------------------------------------------------------------

    /// A runtime error with a message; the position of the running Lua code is added where it is raised.
    pub fn error(&mut self, message: impl AsRef<[u8]>) -> LuaError {
        match self.heap.intern(message.as_ref()) {
            Ok(s) => LuaError::runtime(Value::String(s), Traceback::default(), false),
            Err(e) => e,
        }
    }

    /// Lua's error for a string longer than the longest a script may build.
    pub(crate) fn string_overflow(&mut self) -> LuaError {
        self.error("string length overflow")
    }

    /// Spends fuel for work a native does (bytes copied, pattern steps).
    #[inline]
    pub fn charge_fuel(&mut self, amount: u64) -> Result<(), LuaError> {
        if self.retired {
            return Err(self.error(crate::error::RETIRED_MESSAGE));
        }
        self.heap.fuel.spend(amount)
    }

    pub(crate) fn unwind_scope<T>(
        &mut self,
        run: impl FnOnce(&mut Self) -> Result<T, LuaError>,
    ) -> Result<T, LuaError> {
        let mut scope = UnwindScope {
            vm: self,
            completed: false,
        };
        let result = run(&mut *scope.vm);
        scope.completed = true;
        if scope.vm.retired {
            Err(scope.vm.error(crate::error::RETIRED_MESSAGE))
        } else {
            result
        }
    }

    /// `chunk:line` of a frame's current instruction.
    fn position(proto: &Proto, pc: usize) -> String {
        let line = proto.lines.get(pc).copied().unwrap_or(proto.line);
        format!("{}:{}", proto.chunk, line)
    }

    /// Adds the position (of the running frame at `pc`, else its saved pc) and a traceback to an error not located yet.
    pub(crate) fn locate(&mut self, error: LuaError, pc: Option<usize>) -> LuaError {
        if self.retired {
            return error;
        }
        let value = match &error {
            LuaError::Runtime(thrown) if !thrown.located => thrown.value,
            _ => return error,
        };
        let mut traceback = self.traceback();
        if let (Some(pc), Some((_, at))) = (pc, traceback.0.first_mut()) {
            *at = pc.saturating_sub(1);
        }
        let value = match (value, self.frames.last()) {
            (Value::String(s), Some(frame)) => {
                let at = pc.unwrap_or(frame.pc).saturating_sub(1);
                let position = Self::position(&frame.proto, at);
                match self.prefixed(position, s) {
                    Ok(s) => Value::String(s),
                    Err(e) => return e,
                }
            }
            (value, _) => value,
        };
        LuaError::runtime(value, traceback, true)
    }

    /// Locates an error a tail-called native raised: at its caller's caller, whose call made the frame now gone; with
    /// no position when a native made that call.
    fn locate_tail(&mut self, error: LuaError) -> LuaError {
        if self.retired {
            return error;
        }
        let frame = self.frames.last().expect("a running frame");
        let lua_caller = (!frame.boundary || frame.metamethod.is_some()) && frame.catch.is_none();
        if !lua_caller || self.frames.len() < 2 {
            return self.locate_nowhere(error);
        }
        let value = match &error {
            LuaError::Runtime(thrown) if !thrown.located => thrown.value,
            _ => return error,
        };
        let caller = &self.frames[self.frames.len() - 2];
        let value = match value {
            Value::String(s) => {
                let position = Self::position(&caller.proto, caller.pc.saturating_sub(1));
                match self.prefixed(position, s) {
                    Ok(s) => Value::String(s),
                    Err(e) => return e,
                }
            }
            value => value,
        };
        LuaError::runtime(value, self.traceback(), true)
    }

    /// An error message with its position in front, as a new string: copying and hashing the message (a script's
    /// string, of any length) is paid first, at sixteen bytes a fuel each.
    fn prefixed(&mut self, position: String, message: StrId) -> Result<StrId, LuaError> {
        let length = self.heap.string(message).len();
        self.charge_fuel(2 * (length / 16) as u64)?;
        crate::work::visit(crate::work::Work::Bytes, 2 * length);
        let mut text = position.into_bytes();
        text.extend_from_slice(b": ");
        text.extend_from_slice(self.heap.string(message));
        self.heap.intern(&text)
    }

    /// Marks an error as located without adding a position (raised by a native that no Lua code called directly).
    pub(crate) fn locate_nowhere(&mut self, error: LuaError) -> LuaError {
        if self.retired {
            return error;
        }
        match error {
            LuaError::Runtime(thrown) => LuaError::runtime(thrown.value, self.traceback(), true),
            other => other,
        }
    }

    /// The `chunk:line: ` prefix of the function `level` calls up from the running native (1: its caller), or nothing
    /// when that is a native or there is none. A tail-called native's caller is its caller's caller: the calling
    /// frame is gone.
    pub(crate) fn native_where(&self, level: i64) -> Vec<u8> {
        let level = match self.call_site {
            CallSite::Call { .. } | CallSite::Meta { .. } => level,
            CallSite::Tail if level > 0 => level + 1,
            CallSite::Tail => level,
            CallSite::Native | CallSite::None => level - 1,
        };
        if level <= 0 {
            return Vec::new();
        }
        self.where_(level as usize)
    }

    /// The `chunk:line: ` prefix for `error(message, level)`: level 1 is the function calling `error`.
    pub(crate) fn where_(&self, level: usize) -> Vec<u8> {
        if level == 0 {
            return Vec::new();
        }
        let mut frames = self.frames.iter().rev();
        let mut frame = frames.next();
        for _ in 1..level {
            // A frame a native called (a boundary, or a pcall's function) has a native one level up: no position.
            match frame {
                Some(f) if (!f.boundary || f.metamethod.is_some()) && f.catch.is_none() => {
                    frame = frames.next()
                }
                _ => return Vec::new(),
            }
        }
        match frame {
            Some(frame) => format!(
                "{}: ",
                Self::position(&frame.proto, frame.pc.saturating_sub(1))
            )
            .into_bytes(),
            None => Vec::new(),
        }
    }

    /// `debug.traceback`'s lines for the Lua frames from `level` (1 is the innermost), as LuaJIT's `luaL_traceback`
    /// writes them: `chunk:line: in function 'name'` (the name its caller's call gives it), `in main chunk`, or
    /// `in function <chunk:line>`; when there are more than 22, the first 12, `...` and the last 10. A frame that
    /// tail-called the running native is gone, as in LuaJIT.
    pub(crate) fn traceback_text(&mut self, level: usize) -> Vec<u8> {
        const FIRST: usize = 12;
        const LAST: usize = 10;
        let gone = usize::from(self.call_site == CallSite::Tail);
        let shown: Vec<usize> = (0..self.frames.len())
            .rev()
            .skip(level.saturating_sub(1) + gone)
            .collect();
        let mut text = Vec::new();
        for (i, &k) in shown.iter().enumerate() {
            if shown.len() > FIRST + LAST && i >= FIRST && i < shown.len() - LAST {
                if i == FIRST {
                    text.extend_from_slice(b"\n\t...");
                }
                continue;
            }
            let frame = &self.frames[k];
            let proto = frame.proto.clone();
            let at = Self::position(&proto, frame.pc.saturating_sub(1));
            text.extend_from_slice(format!("\n\t{at}: ").as_bytes());
            match self.frame_name(k) {
                Some(name) => {
                    text.extend_from_slice(b"in function '");
                    text.extend_from_slice(&name);
                    text.push(b'\'');
                }
                None if proto.line == 0 => text.extend_from_slice(b"in main chunk"),
                None => text.extend_from_slice(
                    format!("in function <{}:{}>", proto.chunk, proto.line).as_bytes(),
                ),
            }
        }
        text
    }

    /// The name the call that runs frame `k` gives its function, when a Lua call does.
    fn frame_name(&mut self, k: usize) -> Option<Vec<u8>> {
        let frame = &self.frames[k];
        if frame.boundary || frame.catch.is_some() {
            return None;
        }
        let caller = &self.frames[k.checked_sub(1)?];
        let call = caller.pc.checked_sub(1)?;
        let Op::Call { func, .. } = caller.proto.code[call] else {
            return None;
        };
        if caller.base + usize::from(func) != frame.slot {
            return None;
        }
        let proto = caller.proto.clone();
        self.describe_register(&proto, call, func)
            .map(|(_, name)| name)
    }

    pub(crate) fn traceback(&self) -> Traceback {
        Traceback(
            self.frames
                .iter()
                .rev()
                .take(TRACEBACK_FRAMES)
                .map(|f| (f.proto.clone(), f.pc.saturating_sub(1)))
                .collect(),
        )
    }

    // --- the stack -------------------------------------------------------------------------------------------

    /// Makes the stack exactly `len` long (new slots nil), counting what it grows by.
    #[inline]
    pub(crate) fn set_stack(&mut self, len: usize) -> Result<(), LuaError> {
        if len <= self.stack.len() {
            self.stack.truncate(len);
            return Ok(());
        }
        self.stack.resize(len, Value::Nil);
        self.charge_stack()
    }

    /// Counts what the running coroutine's stack and frames grew by since they were last counted.
    #[inline]
    fn charge_stack(&mut self) -> Result<(), LuaError> {
        let bytes = self.stack.capacity() * SLOT_BYTES + self.frames.capacity() * FRAME_BYTES;
        if bytes > self.stack_charged {
            self.heap.charge(bytes - self.stack_charged)?;
            self.stack_charged = bytes;
        }
        Ok(())
    }

    pub(crate) fn push(&mut self, value: Value) -> Result<(), LuaError> {
        let len = self.stack.len();
        self.set_stack(len + 1)?;
        self.stack[len] = value;
        Ok(())
    }

    /// A zero-based argument, or nil when absent. `args` is valid only during the callback that received it.
    #[inline]
    pub fn arg(&self, args: Args, i: usize) -> Value {
        if self.args_are_live(args) && i < args.count {
            self.stack
                .get(args.base.saturating_add(i))
                .copied()
                .unwrap_or(Value::Nil)
        } else {
            Value::Nil
        }
    }

    // --- objects ---------------------------------------------------------------------------------------------

    pub(crate) fn metatable(&self, value: Value) -> Option<TableId> {
        match value {
            Value::Table(t) => self.heap.table(t).metatable,
            Value::Userdata(u) => self.heap.userdata.get(u.0).metatable,
            Value::String(_) => self.string_meta,
            _ => None,
        }
    }

    #[inline]
    pub(crate) fn metamethod(&self, value: Value, name: StrId) -> Value {
        match self.metatable(value) {
            Some(mt) => self.heap.raw_get(mt, Value::String(name)),
            None => Value::Nil,
        }
    }

    /// Interns bytes under the string-length, fuel and memory bounds. Pin the result before reentry if needed.
    pub fn new_string(&mut self, bytes: &[u8]) -> Result<Value, LuaError> {
        if bytes.len() > self.limits.string_length {
            return Err(self.string_overflow());
        }
        // Making a string (hashing, interning, collecting it later) costs more than an instruction.
        self.charge_fuel(ALLOCATION_FUEL + (bytes.len() / 16) as u64)?;
        crate::work::visit(crate::work::Work::Bytes, bytes.len());
        Ok(Value::String(self.heap.intern(bytes)?))
    }

    /// A number as a string (for concatenation and `tostring`).
    pub(crate) fn number_string(&mut self, n: f64) -> Result<StrId, LuaError> {
        self.heap.intern(number::format(n).as_bytes())
    }

    /// A value as a number, converting numeric strings, as arithmetic does.
    #[inline]
    pub(crate) fn coerce_number(&mut self, value: Value) -> Result<Option<f64>, LuaError> {
        Ok(match value {
            Value::Number(n) => Some(n),
            Value::String(s) => {
                // Reading a numeric string is work in its length: paid before it is read.
                let length = self.heap.string(s).len();
                self.charge_fuel(1 + length as u64 / 16)?;
                crate::work::visit(crate::work::Work::Bytes, length);
                number::parse(self.heap.string(s))
            }
            _ => None,
        })
    }

    pub(crate) fn raw_set_checked(
        &mut self,
        t: TableId,
        key: Value,
        value: Value,
    ) -> Result<(), LuaError> {
        match key {
            Value::Nil => Err(self.error("table index is nil")),
            Value::Number(n) if n.is_nan() => Err(self.error("table index is NaN")),
            _ => {
                self.charge_fuel(1)?;
                self.heap.raw_set_budgeted(t, key, value)
            }
        }
    }

    /// What a type error calls a value: its type name.
    fn type_of(value: Value) -> &'static str {
        value.type_name()
    }

    // --- metamethod-aware operations -------------------------------------------------------------------------

    /// `object[key]`, with `__index`.
    pub(crate) fn index(&mut self, object: Value, key: Value) -> Result<Value, LuaError> {
        let mut object = object;
        for _ in 0..100 {
            let handler = match object {
                Value::Table(t) => {
                    let value = self.heap.raw_get(t, key);
                    if value != Value::Nil {
                        return Ok(value);
                    }
                    match self.heap.table(t).metatable {
                        Some(mt) => self.heap.raw_get(mt, Value::String(self.names.index)),
                        None => return Ok(Value::Nil),
                    }
                }
                _ => {
                    let handler = self.metamethod(object, self.names.index);
                    if handler == Value::Nil {
                        return Err(self.error(format!(
                            "attempt to index a {} value",
                            Self::type_of(object)
                        )));
                    }
                    handler
                }
            };
            match handler {
                Value::Nil => return Ok(Value::Nil),
                Value::Function(_) => {
                    return self.call_metamethod(handler, &[object, key], "__index");
                }
                _ => object = handler,
            }
            self.charge_fuel(1)?;
        }
        Err(self.error("loop in gettable"))
    }

    /// `object[key] = value`, with `__newindex`.
    pub(crate) fn set_index(
        &mut self,
        object: Value,
        key: Value,
        value: Value,
    ) -> Result<(), LuaError> {
        let mut object = object;
        for _ in 0..100 {
            let handler = match object {
                Value::Table(t) => {
                    let present = self.heap.raw_get(t, key) != Value::Nil;
                    let handler = match self.heap.table(t).metatable {
                        Some(mt) if !present => {
                            self.heap.raw_get(mt, Value::String(self.names.newindex))
                        }
                        _ => Value::Nil,
                    };
                    if handler == Value::Nil {
                        return self.raw_set_checked(t, key, value);
                    }
                    handler
                }
                _ => {
                    let handler = self.metamethod(object, self.names.newindex);
                    if handler == Value::Nil {
                        return Err(self.error(format!(
                            "attempt to index a {} value",
                            Self::type_of(object)
                        )));
                    }
                    handler
                }
            };
            if let Value::Function(_) = handler {
                self.call_metamethod(handler, &[object, key, value], "__newindex")?;
                return Ok(());
            }
            object = handler;
            self.charge_fuel(1)?;
        }
        Err(self.error("loop in settable"))
    }

    fn arith_event(&self, op: ArithOp) -> StrId {
        match op {
            ArithOp::Add => self.names.add,
            ArithOp::Sub => self.names.sub,
            ArithOp::Mul => self.names.mul,
            ArithOp::Div => self.names.div,
            ArithOp::Mod => self.names.modulo,
            ArithOp::Pow => self.names.pow,
        }
    }

    fn arith_event_name(op: ArithOp) -> &'static str {
        match op {
            ArithOp::Add => "__add",
            ArithOp::Sub => "__sub",
            ArithOp::Mul => "__mul",
            ArithOp::Div => "__div",
            ArithOp::Mod => "__mod",
            ArithOp::Pow => "__pow",
        }
    }

    #[inline]
    fn arith_numbers(op: ArithOp, a: f64, b: f64) -> f64 {
        match op {
            ArithOp::Add => number::add(a, b),
            ArithOp::Sub => number::sub(a, b),
            ArithOp::Mul => number::mul(a, b),
            ArithOp::Div => number::div(a, b),
            ArithOp::Mod => number::modulo(a, b),
            ArithOp::Pow => number::pow(a, b),
        }
    }

    #[inline(never)]
    pub(crate) fn arith(&mut self, op: ArithOp, a: Value, b: Value) -> Result<Value, LuaError> {
        if let (Some(x), Some(y)) = (self.coerce_number(a)?, self.coerce_number(b)?) {
            return Ok(Value::Number(Self::arith_numbers(op, x, y)));
        }
        let event = self.arith_event(op);
        let mut handler = self.metamethod(a, event);
        if handler == Value::Nil {
            handler = self.metamethod(b, event);
        }
        if handler == Value::Nil {
            let culprit = if self.coerce_number(a)?.is_some() {
                b
            } else {
                a
            };
            return Err(self.error(format!(
                "attempt to perform arithmetic on a {} value",
                Self::type_of(culprit)
            )));
        }
        self.call_metamethod(handler, &[a, b], Self::arith_event_name(op))
    }

    pub(crate) fn negate(&mut self, a: Value) -> Result<Value, LuaError> {
        if let Some(x) = self.coerce_number(a)? {
            return Ok(Value::Number(number::neg(x)));
        }
        let handler = self.metamethod(a, self.names.unm);
        if handler == Value::Nil {
            return Err(self.error(format!(
                "attempt to perform arithmetic on a {} value",
                Self::type_of(a)
            )));
        }
        self.call_metamethod(handler, &[a, a], "__unm")
    }

    pub(crate) fn length(&mut self, a: Value) -> Result<Value, LuaError> {
        match a {
            Value::String(s) => Ok(Value::Number(self.heap.string(s).len() as f64)),
            Value::Table(t) => Ok(Value::Number(self.heap.table(t).border() as f64)),
            _ => {
                let handler = self.metamethod(a, self.names.len);
                if handler == Value::Nil {
                    return Err(self.error(format!(
                        "attempt to get length of a {} value",
                        Self::type_of(a)
                    )));
                }
                self.call_metamethod(handler, &[a, Value::Nil], "__len")
            }
        }
    }

    /// Raw equality: numbers by value, everything else by identity (strings are interned).
    #[inline]
    pub(crate) fn raw_equal(a: Value, b: Value) -> bool {
        a == b
    }

    pub(crate) fn equal(&mut self, a: Value, b: Value) -> Result<bool, LuaError> {
        if a == b {
            return Ok(true);
        }
        match (a, b) {
            (Value::Table(_), Value::Table(_)) | (Value::Userdata(_), Value::Userdata(_)) => {
                let first = self.metamethod(a, self.names.eq);
                if first == Value::Nil || first != self.metamethod(b, self.names.eq) {
                    return Ok(false);
                }
                Ok(self.call_metamethod(first, &[a, b], "__eq")?.is_truthy())
            }
            _ => Ok(false),
        }
    }

    fn compare_error(&mut self, a: Value, b: Value) -> LuaError {
        let (x, y) = (Self::type_of(a), Self::type_of(b));
        if x == y {
            self.error(format!("attempt to compare two {x} values"))
        } else {
            self.error(format!("attempt to compare {x} with {y}"))
        }
    }

    fn order_handler(&self, a: Value, b: Value, event: StrId) -> Value {
        let first = self.metamethod(a, event);
        if first != Value::Nil
            && first == self.metamethod(b, event)
            && std::mem::discriminant(&a) == std::mem::discriminant(&b)
        {
            first
        } else {
            Value::Nil
        }
    }

    /// Pays for comparing two strings: up to the shorter one's bytes.
    fn charge_compare(&mut self, x: StrId, y: StrId) -> Result<(), LuaError> {
        let length = self.heap.string(x).len().min(self.heap.string(y).len());
        self.charge_fuel(length as u64 / 16)
    }

    pub(crate) fn less_than(&mut self, a: Value, b: Value) -> Result<bool, LuaError> {
        match (a, b) {
            (Value::Number(x), Value::Number(y)) => Ok(x < y),
            (Value::String(x), Value::String(y)) => {
                self.charge_compare(x, y)?;
                Ok(self.heap.string(x) < self.heap.string(y))
            }
            _ => {
                let handler = self.order_handler(a, b, self.names.lt);
                if handler == Value::Nil {
                    return Err(self.compare_error(a, b));
                }
                Ok(self.call_metamethod(handler, &[a, b], "__lt")?.is_truthy())
            }
        }
    }

    pub(crate) fn less_equal(&mut self, a: Value, b: Value) -> Result<bool, LuaError> {
        match (a, b) {
            (Value::Number(x), Value::Number(y)) => Ok(x <= y),
            (Value::String(x), Value::String(y)) => {
                self.charge_compare(x, y)?;
                Ok(self.heap.string(x) <= self.heap.string(y))
            }
            _ => {
                let handler = self.order_handler(a, b, self.names.le);
                if handler != Value::Nil {
                    return Ok(self.call_metamethod(handler, &[a, b], "__le")?.is_truthy());
                }
                let handler = self.order_handler(b, a, self.names.lt);
                if handler != Value::Nil {
                    return Ok(!self.call_metamethod(handler, &[b, a], "__lt")?.is_truthy());
                }
                Err(self.compare_error(a, b))
            }
        }
    }

    /// `R[first] .. ... .. R[last]`, right to left as Lua 5.1 does, joining runs of strings and numbers at once.
    /// Concatenates the stack's `first..=last`, right to left as Lua does. When an operand cannot be concatenated,
    /// `culprit` gets its slot, for the message to name it.
    fn concat(
        &mut self,
        first: usize,
        last: usize,
        culprit: &mut Option<usize>,
    ) -> Result<Value, LuaError> {
        let mut accumulated = self.stack[last];
        let mut i = last;
        while i > first {
            let left = self.stack[i - 1];
            let stringy = |v: Value| matches!(v, Value::String(_) | Value::Number(_));
            if stringy(left) && stringy(accumulated) {
                let mut start = i - 1;
                while start > first && stringy(self.stack[start - 1]) {
                    start -= 1;
                }
                let mut bytes = Vec::new();
                for j in start..i {
                    self.append_stringy(&mut bytes, self.stack[j])?;
                }
                self.append_stringy(&mut bytes, accumulated)?;
                accumulated = self.new_string(&bytes)?;
                i = start;
            } else {
                let mut handler = self.metamethod(left, self.names.concat);
                if handler == Value::Nil {
                    handler = self.metamethod(accumulated, self.names.concat);
                }
                if handler == Value::Nil {
                    let (slot, value) = if stringy(left) {
                        (i, accumulated)
                    } else {
                        (i - 1, left)
                    };
                    *culprit = Some(slot);
                    return Err(self.error(format!(
                        "attempt to concatenate a {} value",
                        Self::type_of(value)
                    )));
                }
                accumulated = self.call_metamethod(handler, &[left, accumulated], "__concat")?;
                i -= 1;
            }
        }
        Ok(accumulated)
    }

    fn append_stringy(&mut self, bytes: &mut Vec<u8>, value: Value) -> Result<(), LuaError> {
        match value {
            Value::String(s) => {
                let length = self.heap.string(s).len();
                if bytes.len() + length > self.limits.string_length {
                    return Err(self.string_overflow());
                }
                self.charge_fuel(length as u64 / 16)?;
                bytes.extend_from_slice(self.heap.string(s));
            }
            Value::Number(n) => bytes.extend_from_slice(number::format(n).as_bytes()),
            _ => unreachable!("only strings and numbers are appended"),
        }
        Ok(())
    }

    // --- calls -----------------------------------------------------------------------------------------------

    /// Calls a metamethod (or any function) from inside an instruction with these arguments; its first result.
    pub(crate) fn call_meta(&mut self, function: Value, args: &[Value]) -> Result<Value, LuaError> {
        self.call_as(function, args, CallSite::Native)
    }

    /// Calls the handler of a metamethod `event`: as `call_meta`, the handler (if a native) named for the event when
    /// an instruction set it off, as LuaJIT names it.
    fn call_metamethod(
        &mut self,
        handler: Value,
        args: &[Value],
        event: &'static str,
    ) -> Result<Value, LuaError> {
        // Set off by a native (`tostring`, `table.concat`), it is a call from that native.
        let site = if self.call_site == CallSite::None {
            CallSite::Meta { event }
        } else {
            CallSite::Native
        };
        self.call_as(handler, args, site)
    }

    fn call_as(
        &mut self,
        function: Value,
        args: &[Value],
        site: CallSite,
    ) -> Result<Value, LuaError> {
        let slot = self.stack.len();
        self.push(function)?;
        for &arg in args {
            self.push(arg)?;
        }
        let count = self.call_value_as(slot, args.len(), site)?;
        let result = if count > 0 {
            self.stack[slot]
        } else {
            Value::Nil
        };
        self.stack.truncate(slot);
        Ok(result)
    }

    /// Calls the function at `slot` with the `count` arguments after it; its results end up from `slot` on, the stack
    /// ends after them, and their number is returned.
    pub(crate) fn call_value(&mut self, slot: usize, count: usize) -> Result<usize, LuaError> {
        self.call_value_as(slot, count, CallSite::Native)
    }

    /// `call_value`, a native callee being called as `site` says.
    fn call_value_as(
        &mut self,
        slot: usize,
        count: usize,
        site: CallSite,
    ) -> Result<usize, LuaError> {
        if self.retired {
            return Err(self.error(crate::error::RETIRED_MESSAGE));
        }
        self.stack.truncate(slot + 1 + count);
        let (function, count) = self.callable(slot, count)?;
        match self.heap.functions.get(function.0) {
            Function::Native { .. } => {
                // A native calling a native pays as a call instruction would: the callee may do no paid work at all,
                // so a native looping over callbacks (`table.foreachi(t, type)`) would otherwise run for free.
                if site == CallSite::Native {
                    self.charge_fuel(1)?;
                    crate::work::visit(crate::work::Work::Steps, 1);
                }
                self.call_native(function, slot, count, site)
            }
            Function::Lua { .. } => {
                let previous_native_depth = self.native_depth;
                self.enter_native()?;
                let depth = self.frames.len();
                let caller = std::mem::replace(&mut self.call_site, CallSite::None);
                let result = self
                    .push_frame(function, slot, count, -1, true)
                    .and_then(|()| {
                        if let CallSite::Meta { event } = site {
                            self.frames
                                .last_mut()
                                .expect("the frame just pushed")
                                .metamethod = Some(event);
                        }
                        self.execute(depth)
                    });
                self.call_site = caller;
                self.native_depth = previous_native_depth;
                match result {
                    Ok(Exec::Returned) => {
                        debug_assert_eq!(self.frames.len(), depth, "a returned call left no frame");
                        Ok(self.top - slot)
                    }
                    Ok(Exec::Yielded) => unreachable!("a yield across a native call is refused"),
                    // The failed call's frames stay for whoever catches the error: its handler sees them, then it
                    // drops them (`protect_with`, `execute`, or the host's call).
                    Err(e) => Err(e),
                }
            }
        }
    }

    fn enter_native(&mut self) -> Result<(), LuaError> {
        self.native_depth += 1;
        if self.native_depth > MAX_NATIVE_DEPTH {
            self.native_depth -= 1;
            return Err(self.error("C stack overflow"));
        }
        Ok(())
    }

    /// The function to call at `slot`, going through `__call` (which shifts the arguments right by one).
    fn callable(&mut self, slot: usize, count: usize) -> Result<(FuncId, usize), LuaError> {
        for count in (count..).take(100) {
            let value = self.stack[slot];
            if let Value::Function(f) = value {
                return Ok((f, count));
            }
            let handler = self.metamethod(value, self.names.call);
            if handler == Value::Nil {
                return Err(self.call_error(value));
            }
            self.stack.insert(slot, handler);
            let len = self.stack.len();
            self.set_stack(len)?;
        }
        Err(self.error("loop in __call"))
    }

    fn call_error(&mut self, value: Value) -> LuaError {
        self.error(format!("attempt to call a {} value", Self::type_of(value)))
    }

    /// Calls a native function at `slot` with `count` arguments; moves its results to `slot`.
    fn call_native(
        &mut self,
        function: FuncId,
        slot: usize,
        count: usize,
        site: CallSite,
    ) -> Result<usize, LuaError> {
        let Function::Native {
            function: native, ..
        } = self.heap.functions.get(function.0)
        else {
            unreachable!("a native")
        };
        let native = native.clone();
        let serial = self
            .next_serial
            .checked_add(1)
            .ok_or_else(|| self.error("callback invocation limit exceeded"))?;
        self.next_serial = serial;
        let previous_native_depth = self.native_depth;
        self.enter_native()?;
        let previous_serial = std::mem::replace(&mut self.active_serial, serial);
        let previous_host_depth = self.host_depth;
        if matches!(native, NativeFn::Host(_)) {
            self.host_depth += 1;
        }
        let caller = std::mem::replace(&mut self.call_site, site);
        self.stack.truncate(slot + 1 + count);
        let args = Args {
            owner: self.owner,
            serial,
            base: slot + 1,
            count,
        };
        let result_start = self.stack.len();
        let result = self.unwind_scope(|vm| match &native {
            NativeFn::Builtin(f) => f(vm, args),
            NativeFn::Host(f) => f(vm, args),
        });
        let result = match result {
            Err(LuaError::Runtime(thrown)) if matches!(native, NativeFn::Host(_)) => self
                .validate(thrown.value)
                .and(Err(LuaError::Runtime(thrown))),
            other => other,
        };
        self.native_depth = previous_native_depth;
        self.call_site = caller;
        self.active_serial = previous_serial;
        self.host_depth = previous_host_depth;
        let results = result?;
        if matches!(native, NativeFn::Host(_))
            && results > self.stack.len().saturating_sub(result_start)
        {
            return Err(self.error("invalid host result count"));
        }
        let start = self.stack.len() - results;
        self.stack.copy_within(start.., slot);
        self.stack.truncate(slot + results);
        self.top = slot + results;
        Ok(results)
    }

    /// The upvalues (cells) and closure data for the native being called: its own upvalues.
    pub(crate) fn native_upvalue(&self, args: Args, i: usize) -> Value {
        let Value::Function(f) = self.stack[args.base - 1] else {
            return Value::Nil;
        };
        match self.heap.functions.get(f.0) {
            Function::Native { upvalues, .. } => upvalues.get(i).copied().unwrap_or(Value::Nil),
            Function::Lua { .. } => Value::Nil,
        }
    }

    /// Pushes a frame for a Lua function at `slot` called with `count` arguments.
    pub(crate) fn push_frame(
        &mut self,
        function: FuncId,
        slot: usize,
        count: usize,
        want: i32,
        boundary: bool,
    ) -> Result<(), LuaError> {
        if self.frames.len() >= self.limits.call_depth as usize {
            return Err(self.error("stack overflow"));
        }
        let Function::Lua {
            proto, upvalues, ..
        } = self.heap.functions.get(function.0)
        else {
            unreachable!("a Lua function")
        };
        let (proto, upvalues) = (proto.clone(), upvalues.clone());
        let params = usize::from(proto.params);
        let given = count.min(params);
        let first = slot + 1;
        let (base, varargs) = if proto.vararg {
            let base = first + count;
            self.set_stack(base + given)?;
            for i in 0..given {
                self.stack[base + i] = self.stack[first + i];
            }
            (
                base,
                (first + params.min(count), count.saturating_sub(params)),
            )
        } else {
            (first, (0, 0))
        };
        self.stack.truncate(base + given);
        self.set_stack(base + usize::from(proto.registers))?;
        self.frames.push(Frame {
            function,
            proto,
            upvalues,
            pc: 0,
            base,
            slot,
            want,
            varargs,
            boundary,
            catch: None,
            metamethod: None,
        });
        self.charge_stack()?;
        Ok(())
    }

    /// Puts `count` results from `from` at `slot` as a caller wanting `want` expects them, and returns where the
    /// caller's stack must end at least.
    fn place_results(
        &mut self,
        from: usize,
        count: usize,
        slot: usize,
        want: i32,
    ) -> Result<(), LuaError> {
        // Moving many values is work one instruction must not get for free.
        self.charge_fuel((count / VALUES_PER_FUEL) as u64)?;
        if from != slot {
            self.stack.copy_within(from..from + count, slot);
        }
        if want < 0 {
            self.stack.truncate(slot + count);
            self.top = slot + count;
        } else {
            let want = want as usize;
            self.stack.truncate(slot + count.min(want));
            self.set_stack(slot + want)?;
            self.top = slot + want;
        }
        Ok(())
    }

    /// Runs Lua frames until the boundary frame that started this loop (the frame at `boundary`, the innermost) returns
    /// or the coroutine yields; an error stops at the innermost `pcall` this loop runs.
    pub(crate) fn execute(&mut self, boundary: usize) -> Result<Exec, LuaError> {
        // This loop runs the frames from its boundary up. A native's failed call leaves its frames above them (none
        // of which catches), for the handler of the frame that catches to see; that frame drops them. The caller
        // knows the boundary: finding it would walk every frame of a deep coroutine at each resume.
        assert!(
            self.frames[boundary].boundary,
            "execution starts at a boundary frame"
        );
        debug_assert!(
            self.frames[boundary + 1..].iter().all(|f| !f.boundary),
            "the boundary is the innermost"
        );
        loop {
            if self.retired {
                return Err(self.error(crate::error::RETIRED_MESSAGE));
            }
            let result = self.run();
            if self.retired {
                return Err(self.error(crate::error::RETIRED_MESSAGE));
            }
            match result {
                Err(LuaError::Runtime(thrown)) => self.catch(boundary, thrown)?,
                other => return other,
            }
        }
    }

    /// Hands an error to the innermost `pcall` this loop runs (its handler runs first, where the error happened),
    /// which then returns `false` and the error; with none, the error goes on out. Out of line: every native re-entry
    /// nests `execute`, and this keeps its frame small.
    #[inline(never)]
    fn catch(&mut self, boundary: usize, thrown: Box<Thrown>) -> Result<(), LuaError> {
        let Some(at) = self.frames[boundary..]
            .iter()
            .rposition(|f| f.catch.is_some())
        else {
            return Err(LuaError::Runtime(thrown));
        };
        let frame = &self.frames[boundary + at];
        let (slot, catch) = (frame.slot, frame.catch.expect("a catching frame"));
        let value = self.handle(catch.handler, thrown.value)?;
        self.frames.truncate(boundary + at);
        self.stack.truncate(slot);
        self.finish_protected(slot - 1, value, catch.want)
    }

    /// What a failed protected call returns besides `false`: the error, or what the `xpcall` handler made of it. The
    /// handler runs where the error happened, the failed call's frames still there (so `debug.traceback` shows them),
    /// as in LuaJIT; an error in the handler is "error in error handling".
    pub(crate) fn handle(&mut self, handler: Value, error: Value) -> Result<Value, LuaError> {
        if self.retired {
            return Err(self.error(crate::error::RETIRED_MESSAGE));
        }
        if handler == Value::Nil {
            return Ok(error);
        }
        match self.protect(|vm| vm.call_meta(handler, &[error])) {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(_)) => Ok(Value::String(self.heap.intern(b"error in error handling")?)),
            Err(limit) => Err(limit),
        }
    }

    /// Runs `f`, turning a Lua error into Ok(Err(value)) and restoring the frames and nesting; limits pass through.
    pub(crate) fn protect<T>(
        &mut self,
        f: impl FnOnce(&mut Vm) -> Result<T, LuaError>,
    ) -> Result<Result<T, Value>, LuaError> {
        self.protect_with(Value::Nil, f)
    }

    /// [`protect`](Self::protect) with a message handler, which runs on the error before the failed call's frames
    /// are dropped.
    pub(crate) fn protect_with<T>(
        &mut self,
        handler: Value,
        f: impl FnOnce(&mut Vm) -> Result<T, LuaError>,
    ) -> Result<Result<T, Value>, LuaError> {
        let depth = self.frames.len();
        let native_depth = self.native_depth;
        let stack = self.stack.len();
        if self.retired {
            return Err(self.error(crate::error::RETIRED_MESSAGE));
        }
        let result = f(self);
        if self.retired {
            return Err(self.error(crate::error::RETIRED_MESSAGE));
        }
        match result {
            Ok(value) => Ok(Ok(value)),
            Err(LuaError::Runtime(thrown)) => {
                let value = self.handle(handler, thrown.value);
                self.frames.truncate(depth);
                self.native_depth = native_depth;
                self.stack.truncate(stack);
                value.map(Err)
            }
            Err(limit) => Err(limit),
        }
    }

    /// Puts `false, value` where the protected call sat, as its caller wants, and gives the caller its registers back.
    fn finish_protected(&mut self, slot: usize, value: Value, want: i32) -> Result<(), LuaError> {
        self.stack.truncate(slot);
        self.push(Value::Boolean(false))?;
        self.push(value)?;
        self.place_results(slot, 2, slot, want)?;
        if let Some(caller) = self.frames.last() {
            let top = caller.base + usize::from(caller.proto.registers);
            if self.stack.len() < top {
                self.set_stack(top)?;
            }
        }
        Ok(())
    }

    /// Whether a native is `pcall` (Some(false)) or `xpcall` (Some(true)).
    fn protected_kind(&self, function: FuncId) -> Option<bool> {
        match self.heap.functions.get(function.0) {
            Function::Native {
                function: NativeFn::Builtin(_),
                name: "pcall",
                ..
            } => Some(false),
            Function::Native {
                function: NativeFn::Builtin(_),
                name: "xpcall",
                ..
            } => Some(true),
            _ => None,
        }
    }

    /// `pcall(f, ...)` or `xpcall(f, handler, ...)` called from Lua at `slot`: a Lua `f` runs as a frame that catches
    /// errors (so a coroutine may yield inside it), returning true; a native `f` runs here, returning false.
    fn protected(
        &mut self,
        slot: usize,
        count: usize,
        want: i32,
        xpcall: bool,
    ) -> Result<Protected, LuaError> {
        let name = if xpcall { "xpcall" } else { "pcall" };
        let needed = if xpcall { 2 } else { 1 };
        if count < needed {
            let message = format!("bad argument #{} to '{name}' (value expected)", count + 1);
            return Err(self.error(message));
        }
        let handler = if xpcall {
            self.stack.remove(slot + 2)
        } else {
            Value::Nil
        };
        let count = count + 1 - needed;
        let (function, count) =
            match self.protect_with(handler, |vm| vm.callable(slot + 1, count - 1))? {
                Ok(called) => called,
                Err(value) => {
                    self.finish_protected(slot, value, want)?;
                    return Ok(Protected::Done);
                }
            };
        let inner_want = if want < 0 { -1 } else { (want - 1).max(0) };
        if matches!(self.heap.functions.get(function.0), Function::Lua { .. }) {
            self.push_frame(function, slot + 1, count, inner_want, false)?;
            self.frames.last_mut().expect("the protected frame").catch =
                Some(Catch { handler, want });
            return Ok(Protected::Frame);
        }
        match self.protect_with(handler, |vm| {
            vm.call_native(function, slot + 1, count, CallSite::Native)
        })? {
            Ok(_) if self.yielding => {
                self.yielding = false;
                let thread = self.threads.get_mut(self.current.0);
                thread.pending = Some(Pending {
                    values: slot + 1,
                    slot,
                    want,
                    protected: true,
                });
                return Ok(Protected::Yielded);
            }
            Ok(produced) => {
                self.stack[slot] = Value::Boolean(true);
                self.place_results(slot, produced + 1, slot, want)?;
            }
            Err(value) => self.finish_protected(slot, value, want)?,
        }
        Ok(Protected::Done)
    }

    /// The loop itself.
    fn run(&mut self) -> Result<Exec, LuaError> {
        'frame: loop {
            let frame = self.frames.last().expect("a running frame");
            let proto = frame.proto.clone();
            let base = frame.base;
            let mut pc = frame.pc;
            let code = &proto.code[..];
            let frame_top = base + usize::from(proto.registers);
            loop {
                if self.heap.fuel.left == 0 {
                    self.frames.last_mut().expect("a running frame").pc = pc;
                    return Err(LuaError::Limit(self.heap.fuel.limit));
                }
                self.heap.fuel.left -= 1;
                if self.heap.phase != Phase::Idle
                    || self.heap.memory >= self.gc_threshold
                    || self.gc_requested
                {
                    self.frames.last_mut().expect("a running frame").pc = pc;
                    located!(self, self.gc_point(), pc);
                }
                let op = code[pc];
                pc += 1;
                match op {
                    Op::Move { a, b } => {
                        self.stack[base + usize::from(a)] = self.stack[base + usize::from(b)]
                    }
                    Op::LoadK { a, k } => {
                        self.stack[base + usize::from(a)] = proto.constants[k as usize]
                    }
                    Op::LoadBool { a, value, skip } => {
                        self.stack[base + usize::from(a)] = Value::Boolean(value);
                        if skip {
                            pc += 1;
                        }
                    }
                    Op::LoadNil { a, count } => {
                        let a = base + usize::from(a);
                        self.stack[a..=a + usize::from(count)].fill(Value::Nil);
                    }
                    Op::NewCell { a } => {
                        let value = self.stack[base + usize::from(a)];
                        let cell = located!(self, self.heap.new_cell(value), pc);
                        self.stack[base + usize::from(a)] = Value::Cell(cell);
                    }
                    Op::GetCell { a, cell } => {
                        let Value::Cell(cell) = self.stack[base + usize::from(cell)] else {
                            unreachable!("a cell register")
                        };
                        self.stack[base + usize::from(a)] = *self.heap.cells.get(cell.0);
                    }
                    Op::SetCell { cell, b } => {
                        let Value::Cell(cell) = self.stack[base + usize::from(cell)] else {
                            unreachable!("a cell register")
                        };
                        self.heap.set_cell(cell, self.stack[base + usize::from(b)]);
                    }
                    Op::GetUpvalue { a, upvalue } => {
                        let cell = self.frames.last().expect("a running frame").upvalues
                            [usize::from(upvalue)];
                        self.stack[base + usize::from(a)] = *self.heap.cells.get(cell.0);
                    }
                    Op::SetUpvalue { upvalue, b } => {
                        let cell = self.frames.last().expect("a running frame").upvalues
                            [usize::from(upvalue)];
                        self.heap.set_cell(cell, self.stack[base + usize::from(b)]);
                    }
                    Op::GetGlobal { a, k } => {
                        let env = self.running_env();
                        let key = proto.constants[k as usize];
                        let value = self.heap.raw_get(env, key);
                        let value =
                            if value == Value::Nil && self.heap.table(env).metatable.is_some() {
                                save_pc!(self, pc);
                                located!(self, self.index(Value::Table(env), key), pc)
                            } else {
                                value
                            };
                        self.stack[base + usize::from(a)] = value;
                    }
                    Op::SetGlobal { b, k } => {
                        let env = self.running_env();
                        let key = proto.constants[k as usize];
                        let value = self.stack[base + usize::from(b)];
                        if self.heap.table(env).metatable.is_none() {
                            located!(self, self.raw_set_checked(env, key, value), pc);
                        } else {
                            save_pc!(self, pc);
                            located!(self, self.set_index(Value::Table(env), key, value), pc);
                        }
                    }
                    Op::GetTable { a, table, key } => {
                        let object = self.stack[base + usize::from(table)];
                        let key = self.rk(&proto, base, key);
                        let value = match object {
                            Value::Table(t) => {
                                let value = self.heap.raw_get(t, key);
                                if value == Value::Nil && self.heap.table(t).metatable.is_some() {
                                    save_pc!(self, pc);
                                    located!(self, self.index(object, key), pc)
                                } else {
                                    value
                                }
                            }
                            _ => {
                                save_pc!(self, pc);
                                let result = self.index(object, key);
                                let result =
                                    result.map_err(|e| self.name_error(e, &proto, pc - 1, table));
                                located!(self, result, pc)
                            }
                        };
                        self.stack[base + usize::from(a)] = value;
                    }
                    Op::SetTable { table, key, value } => {
                        let object = self.stack[base + usize::from(table)];
                        let key = self.rk(&proto, base, key);
                        let value = self.rk(&proto, base, value);
                        let result = match object {
                            Value::Table(t) if self.heap.table(t).metatable.is_none() => {
                                self.raw_set_checked(t, key, value)
                            }
                            _ => {
                                save_pc!(self, pc);
                                self.set_index(object, key, value)
                                    .map_err(|e| self.name_error(e, &proto, pc - 1, table))
                            }
                        };
                        located!(self, result, pc);
                    }
                    Op::NewTable { a, array, hash } => {
                        located!(self, self.charge_fuel(ALLOCATION_FUEL), pc);
                        let t = located!(
                            self,
                            self.allocate_table(usize::from(array), usize::from(hash)),
                            pc
                        );
                        self.stack[base + usize::from(a)] = Value::Table(t);
                    }
                    Op::SelfMethod { a, object, key } => {
                        let receiver = self.stack[base + usize::from(object)];
                        let key = self.rk(&proto, base, key);
                        save_pc!(self, pc);
                        let result = self
                            .index(receiver, key)
                            .map_err(|e| self.name_error(e, &proto, pc - 1, object));
                        let method = located!(self, result, pc);
                        self.stack[base + usize::from(a) + 1] = receiver;
                        self.stack[base + usize::from(a)] = method;
                    }
                    Op::Arith { op, a, b, c } => {
                        let x = self.rk(&proto, base, b);
                        let y = self.rk(&proto, base, c);
                        let value = match (x, y) {
                            (Value::Number(x), Value::Number(y)) => {
                                Value::Number(Self::arith_numbers(op, x, y))
                            }
                            _ => {
                                save_pc!(self, pc);
                                let result = self.arith(op, x, y);
                                let culprit = match self.coerce_number(x) {
                                    Ok(Some(_)) => c,
                                    Ok(None) => b,
                                    Err(limit) => return Err(limit),
                                };
                                let result = match culprit.0 {
                                    r if r < Rk::CONSTANT => result
                                        .map_err(|e| self.name_error(e, &proto, pc - 1, r as u8)),
                                    _ => result,
                                };
                                located!(self, result, pc)
                            }
                        };
                        self.stack[base + usize::from(a)] = value;
                    }
                    Op::Neg { a, b } => {
                        save_pc!(self, pc);
                        let result = self
                            .negate(self.stack[base + usize::from(b)])
                            .map_err(|e| self.name_error(e, &proto, pc - 1, b));
                        let value = located!(self, result, pc);
                        self.stack[base + usize::from(a)] = value;
                    }
                    Op::Not { a, b } => {
                        self.stack[base + usize::from(a)] =
                            Value::Boolean(!self.stack[base + usize::from(b)].is_truthy());
                    }
                    Op::Len { a, b } => {
                        save_pc!(self, pc);
                        let result = self
                            .length(self.stack[base + usize::from(b)])
                            .map_err(|e| self.name_error(e, &proto, pc - 1, b));
                        let value = located!(self, result, pc);
                        self.stack[base + usize::from(a)] = value;
                    }
                    Op::Concat { a, first, last } => {
                        save_pc!(self, pc);
                        let mut culprit = None;
                        let result = self
                            .concat(
                                base + usize::from(first),
                                base + usize::from(last),
                                &mut culprit,
                            )
                            .map_err(|e| match culprit {
                                Some(slot) => {
                                    self.name_error(e, &proto, pc - 1, (slot - base) as u8)
                                }
                                None => e,
                            });
                        let value = located!(self, result, pc);
                        self.stack[base + usize::from(a)] = value;
                    }
                    Op::Jump { offset } => pc = (pc as i64 + i64::from(offset)) as usize,
                    Op::Compare { op, b, c, expect } => {
                        let x = self.rk(&proto, base, b);
                        let y = self.rk(&proto, base, c);
                        let result = match (op, x, y) {
                            (CompareOp::Lt, Value::Number(x), Value::Number(y)) => x < y,
                            (CompareOp::Le, Value::Number(x), Value::Number(y)) => x <= y,
                            (CompareOp::Eq, _, _) => {
                                save_pc!(self, pc);
                                located!(self, self.equal(x, y), pc)
                            }
                            (CompareOp::Lt, _, _) => {
                                save_pc!(self, pc);
                                located!(self, self.less_than(x, y), pc)
                            }
                            (CompareOp::Le, _, _) => {
                                save_pc!(self, pc);
                                located!(self, self.less_equal(x, y), pc)
                            }
                        };
                        pc = Self::follow_jump(code, pc, result == expect);
                    }
                    Op::Test { a, expect } => {
                        let truthy = self.stack[base + usize::from(a)].is_truthy();
                        pc = Self::follow_jump(code, pc, truthy == expect);
                    }
                    Op::Call {
                        func,
                        args,
                        results,
                    } => {
                        let slot = base + usize::from(func);
                        let count = if args == 0 {
                            self.top - slot - 1
                        } else {
                            usize::from(args) - 1
                        };
                        let want = i32::from(results) - 1;
                        self.frames.last_mut().expect("a running frame").pc = pc;
                        self.stack.truncate(slot + 1 + count);
                        let called = self
                            .callable(slot, count)
                            .map_err(|e| self.name_error(e, &proto, pc - 1, func));
                        let (function, count) = located!(self, called, pc);
                        if matches!(self.heap.functions.get(function.0), Function::Lua { .. }) {
                            located!(
                                self,
                                self.push_frame(function, slot, count, want, false),
                                pc
                            );
                            continue 'frame;
                        }
                        if let Some(xpcall) = self.protected_kind(function) {
                            match located!(self, self.protected(slot, count, want, xpcall), pc) {
                                Protected::Frame => continue 'frame,
                                Protected::Yielded => return Ok(Exec::Yielded),
                                Protected::Done => {}
                            }
                            if self.stack.len() < frame_top {
                                located!(self, self.set_stack(frame_top), pc);
                            }
                            continue;
                        }
                        let site = CallSite::Call { register: func };
                        let produced =
                            located!(self, self.call_native(function, slot, count, site), pc);
                        if self.yielding {
                            self.yielding = false;
                            let thread = self.threads.get_mut(self.current.0);
                            thread.pending = Some(Pending {
                                values: slot,
                                slot,
                                want,
                                protected: false,
                            });
                            return Ok(Exec::Yielded);
                        }
                        located!(self, self.place_results(slot, produced, slot, want), pc);
                        if self.stack.len() < frame_top {
                            located!(self, self.set_stack(frame_top), pc);
                        }
                    }
                    Op::TailCall { func, args } => {
                        let slot = base + usize::from(func);
                        let count = if args == 0 {
                            self.top - slot - 1
                        } else {
                            usize::from(args) - 1
                        };
                        self.frames.last_mut().expect("a running frame").pc = pc;
                        self.stack.truncate(slot + 1 + count);
                        let called = self
                            .callable(slot, count)
                            .map_err(|e| self.name_error(e, &proto, pc - 1, func));
                        let (function, count) = located!(self, called, pc);
                        let frame = self.frames.last().expect("a running frame");
                        let (frame_slot, want, boundary) = (frame.slot, frame.want, frame.boundary);
                        if matches!(self.heap.functions.get(function.0), Function::Lua { .. }) {
                            // Reuse this frame's place: the callee and its arguments move down to its slot, and it
                            // catches errors when this frame did (a pcall's function tail-calling).
                            self.stack.copy_within(slot..slot + 1 + count, frame_slot);
                            self.stack.truncate(frame_slot + 1 + count);
                            let catch = self.frames.pop().expect("a running frame").catch;
                            located!(
                                self,
                                self.push_frame(function, frame_slot, count, want, boundary),
                                pc
                            );
                            self.frames.last_mut().expect("the frame just pushed").catch = catch;
                            continue 'frame;
                        }
                        if let Some(xpcall) = self.protected_kind(function) {
                            // `return pcall(...)`: a plain call for all results; the `Return` after it returns them.
                            match located!(self, self.protected(slot, count, -1, xpcall), pc) {
                                Protected::Frame => continue 'frame,
                                Protected::Yielded => return Ok(Exec::Yielded),
                                Protected::Done => continue,
                            }
                        }
                        let produced = self.call_native(function, slot, count, CallSite::Tail);
                        let produced = match produced {
                            Ok(produced) => produced,
                            Err(error) => return Err(self.locate_tail(error)),
                        };
                        if self.yielding {
                            self.yielding = false;
                            let thread = self.threads.get_mut(self.current.0);
                            thread.pending = Some(Pending {
                                values: slot,
                                slot,
                                want: -1,
                                protected: false,
                            });
                            // The yield's results return from this frame when the coroutine resumes.
                            self.frames.last_mut().expect("a running frame").pc = pc;
                            return Ok(Exec::Yielded);
                        }
                        match self.return_from_frame(slot, produced) {
                            Ok(true) => return Ok(Exec::Returned),
                            Ok(false) => continue 'frame,
                            Err(e) => return Err(e),
                        }
                    }
                    Op::Return { first, count } => {
                        let first = base + usize::from(first);
                        let count = if count == 0 {
                            self.top - first
                        } else {
                            usize::from(count) - 1
                        };
                        match self.return_from_frame(first, count) {
                            Ok(true) => return Ok(Exec::Returned),
                            Ok(false) => continue 'frame,
                            Err(e) => return Err(e),
                        }
                    }
                    Op::ForPrep { a, offset } => {
                        let a = base + usize::from(a);
                        let start = located!(self, self.coerce_number(self.stack[a]), pc);
                        let limit = located!(self, self.coerce_number(self.stack[a + 1]), pc);
                        let step = located!(self, self.coerce_number(self.stack[a + 2]), pc);
                        let (Some(start), Some(limit), Some(step)) = (start, limit, step) else {
                            let message = if start.is_none() {
                                "'for' initial value must be a number"
                            } else if limit.is_none() {
                                "'for' limit must be a number"
                            } else {
                                "'for' step must be a number"
                            };
                            let error = self.error(message);
                            return Err(self.locate(error, Some(pc)));
                        };
                        self.stack[a] = Value::Number(start);
                        self.stack[a + 1] = Value::Number(limit);
                        self.stack[a + 2] = Value::Number(step);
                        if number::for_in_range(start, limit, step) {
                            self.stack[a + 3] = Value::Number(start);
                        } else {
                            pc = (pc as i64 + i64::from(offset)) as usize;
                        }
                    }
                    Op::ForLoop { a, offset } => {
                        let a = base + usize::from(a);
                        let (Value::Number(index), Value::Number(limit), Value::Number(step)) =
                            (self.stack[a], self.stack[a + 1], self.stack[a + 2])
                        else {
                            unreachable!("the loop's prep made numbers")
                        };
                        let index = number::add(index, step);
                        let more = number::for_in_range(index, limit, step);
                        self.stack[a] = Value::Number(index);
                        if more {
                            self.stack[a + 3] = Value::Number(index);
                            pc = (pc as i64 + i64::from(offset)) as usize;
                        }
                    }
                    Op::ForEachLoop { a, offset } => {
                        let a = base + usize::from(a);
                        let control = self.stack[a + 3];
                        if control != Value::Nil {
                            self.stack[a + 2] = control;
                            pc = (pc as i64 + i64::from(offset)) as usize;
                        }
                    }
                    Op::SetList {
                        table,
                        count,
                        start,
                    } => {
                        let t = base + usize::from(table);
                        let count = if count == 0 {
                            self.top - t - 1
                        } else {
                            usize::from(count) - 1
                        };
                        let Value::Table(id) = self.stack[t] else {
                            unreachable!("a table constructor's table")
                        };
                        located!(self, self.charge_fuel((count / VALUES_PER_FUEL) as u64), pc);
                        for i in 0..count {
                            let key = Value::Number((start as usize + i) as f64);
                            let value = self.stack[t + 1 + i];
                            located!(self, self.raw_set_checked(id, key, value), pc);
                        }
                        if self.stack.len() < frame_top {
                            located!(self, self.set_stack(frame_top), pc);
                        }
                    }
                    Op::Closure { a, proto: index } => {
                        let child = proto.protos[index as usize].clone();
                        let f = located!(self, self.create_closure(base, child), pc);
                        self.stack[base + usize::from(a)] = Value::Function(f);
                    }
                    Op::VarArg { a, count } => {
                        let (start, available) =
                            self.frames.last().expect("a running frame").varargs;
                        let a = base + usize::from(a);
                        let wanted = if count == 0 {
                            available
                        } else {
                            usize::from(count) - 1
                        };
                        located!(
                            self,
                            self.charge_fuel((wanted / VALUES_PER_FUEL) as u64),
                            pc
                        );
                        if a + wanted > self.stack.len() {
                            located!(self, self.set_stack(a + wanted), pc);
                        }
                        for i in 0..wanted {
                            self.stack[a + i] = if i < available {
                                self.stack[start + i]
                            } else {
                                Value::Nil
                            };
                        }
                        if count == 0 {
                            self.top = a + wanted;
                        }
                    }
                }
            }
        }
    }

    // Closure construction does not reenter execution; keep its temporaries off recursive execution frames.
    #[inline(never)]
    fn create_closure(&mut self, base: usize, child: Rc<Proto>) -> Result<FuncId, LuaError> {
        // The cells it copies are paid before they are copied: 1 a 8, like any values moved, so that a host's
        // `Limits::function_upvalues` of 255 costs no more per fuel than a closure of a few.
        self.charge_fuel(ALLOCATION_FUEL + (child.captures.len() / VALUES_PER_FUEL) as u64)?;
        let env = self.running_env();
        let (stack, frame) = (&self.stack, self.frames.last().expect("a running frame"));
        let captures = child.captures.len();
        // The cells are gathered once the heap has counted and made room for the function.
        self.heap.new_lua_function(captures, || {
            crate::work::visit(crate::work::Work::Cells, captures);
            let upvalues: Rc<[CellId]> = child
                .captures
                .iter()
                .map(|capture| match *capture {
                    Capture::Register(r) => match stack[base + usize::from(r)] {
                        Value::Cell(cell) => cell,
                        other => unreachable!("a captured local lives in a cell, found {other:?}"),
                    },
                    Capture::Upvalue(u) => frame.upvalues[usize::from(u)],
                })
                .collect();
            Function::Lua {
                proto: child.clone(),
                upvalues,
                env,
            }
        })
    }

    /// The running function's environment, read from the function itself (never a copy that `setfenv` could leave
    /// stale).
    #[inline]
    fn running_env(&self) -> TableId {
        let function = self.frames.last().expect("a running frame").function;
        match self.heap.functions.get(function.0) {
            Function::Lua { env, .. } => *env,
            Function::Native { .. } => unreachable!("a frame runs a Lua function"),
        }
    }

    /// The operand value of an `Rk`.
    #[inline]
    fn rk(&self, proto: &Proto, base: usize, rk: Rk) -> Value {
        if rk.0 >= Rk::CONSTANT {
            proto.constants[usize::from(rk.0 - Rk::CONSTANT)]
        } else {
            self.stack[base + usize::from(rk.0)]
        }
    }

    /// After a compare or test at `pc - 1`: the next instruction is a jump, taken when `jump`.
    #[inline]
    fn follow_jump(code: &[Op], pc: usize, jump: bool) -> usize {
        if jump {
            let Op::Jump { offset } = code[pc] else {
                unreachable!("a test is followed by a jump")
            };
            (pc as i64 + 1 + i64::from(offset)) as usize
        } else {
            pc + 1
        }
    }

    /// What a register held at an instruction, as Lua's messages name it (`("local", "a")`, `("global", "x")`, ...):
    /// a local active there, else what the last instruction before it that wrote the register read, following moves,
    /// as LuaJIT finds it (straight back through the code, jumps and all).
    pub(crate) fn describe_register(
        &mut self,
        proto: &Proto,
        pc: usize,
        register: u8,
    ) -> Option<(&'static str, Vec<u8>)> {
        // The scan walks back through the function, its locals at each register it follows, and copies a name: no
        // further than the fuel left pays for, at eight units a fuel, and then paid. Past that the error goes unnamed
        // and the call ends at its next instruction.
        let limit = self.heap.fuel.affordable(VALUES_PER_FUEL);
        let units = std::cell::Cell::new(1usize);
        let afford = |more: usize| {
            units.set(units.get().saturating_add(more));
            crate::work::visit(crate::work::Work::Slots, more);
            units.get() <= limit
        };
        let named = self.scan_register(proto, pc, register, &afford);
        if units.get() > limit {
            let _ = self.heap.fuel.exhaust();
            return None;
        }
        self.heap.fuel.left -= (units.get() / VALUES_PER_FUEL) as u64;
        named
    }

    /// `describe_register`'s scan; `afford(units)` counts work about to be done, false when it is not paid for.
    fn scan_register(
        &self,
        proto: &Proto,
        pc: usize,
        register: u8,
        afford: &dyn Fn(usize) -> bool,
    ) -> Option<(&'static str, Vec<u8>)> {
        let local_name = |register: u8| {
            if !afford(proto.locals.len()) {
                return None;
            }
            proto
                .locals
                .iter()
                .rev()
                .find(|l| l.register == register && l.start <= pc && pc < l.end)
                .filter(|l| afford(l.name.len()))
                .map(|l| l.name.clone())
        };
        let constant = |k: usize| match proto.constants[k] {
            Value::String(s) if afford(self.heap.string(s).len()) => {
                Some(self.heap.string(s).to_vec())
            }
            _ => None,
        };
        let mut register = register;
        let mut i = pc.min(proto.code.len());
        'register: loop {
            if let Some(name) = local_name(register) {
                return Some(("local", name.into_vec()));
            }
            while i > 0 {
                if !afford(1) {
                    return None;
                }
                i -= 1;
                let r = usize::from(register);
                let within = |first: u8, count: usize| {
                    usize::from(first) <= r && r < usize::from(first) + count
                };
                match proto.code[i] {
                    Op::GetGlobal { a, k } if a == register => {
                        return constant(k as usize).map(|n| ("global", n));
                    }
                    Op::GetTable { a, key, .. } if a == register => {
                        return (key.0 >= Rk::CONSTANT)
                            .then(|| {
                                constant(usize::from(key.0 - Rk::CONSTANT)).map(|n| ("field", n))
                            })
                            .flatten();
                    }
                    Op::SelfMethod { a, key, .. } if a == register => {
                        return (key.0 >= Rk::CONSTANT)
                            .then(|| {
                                constant(usize::from(key.0 - Rk::CONSTANT)).map(|n| ("method", n))
                            })
                            .flatten();
                    }
                    Op::GetUpvalue { a, upvalue } if a == register => {
                        return proto
                            .upvalue_names
                            .get(usize::from(upvalue))
                            .filter(|n| afford(n.len()))
                            .map(|n| ("upvalue", n.to_vec()));
                    }
                    Op::GetCell { a, cell } if a == register => {
                        return local_name(cell).map(|n| ("local", n.into_vec()));
                    }
                    Op::Move { a, b } if a == register => {
                        register = b;
                        continue 'register;
                    }
                    Op::LoadK { a, .. }
                    | Op::LoadBool { a, .. }
                    | Op::NewCell { a }
                    | Op::NewTable { a, .. }
                    | Op::Arith { a, .. }
                    | Op::Neg { a, .. }
                    | Op::Not { a, .. }
                    | Op::Len { a, .. }
                    | Op::Concat { a, .. }
                    | Op::Closure { a, .. }
                        if a == register =>
                    {
                        return None;
                    }
                    Op::SelfMethod { a, .. } if within(a, 2) => return None,
                    Op::LoadNil { a, count } if within(a, usize::from(count) + 1) => return None,
                    Op::ForPrep { a, .. } | Op::ForLoop { a, .. } if within(a, 4) => return None,
                    Op::ForEachLoop { a, .. } if within(a, 4) => return None,
                    // A call or `...` writes its first register and every one above it.
                    Op::Call { func: a, .. }
                    | Op::TailCall { func: a, .. }
                    | Op::VarArg { a, .. }
                        if usize::from(a) <= r =>
                    {
                        return None;
                    }
                    _ => {}
                }
            }
            return None;
        }
    }

    /// The running native as its caller named it (`("global", "f")`, `("method", "rep")`, `("metamethod",
    /// "__index")`), or None when the caller gave it no name ('?' in messages), as LuaJIT names it.
    pub(crate) fn callee(&mut self) -> Option<(&'static str, Vec<u8>)> {
        let frame = self.frames.last()?;
        match self.call_site {
            CallSite::Call { register } => {
                let (proto, pc) = (frame.proto.clone(), frame.pc.checked_sub(1)?);
                self.describe_register(&proto, pc, register)
            }
            CallSite::Tail => {
                if frame.catch.is_some() || self.frames.len() < 2 {
                    return None;
                }
                // A metamethod's frame was set off by an instruction: the native is named for the event.
                if frame.boundary {
                    return frame
                        .metamethod
                        .map(|event| ("metamethod", event.as_bytes().to_vec()));
                }
                let caller = &self.frames[self.frames.len() - 2];
                let (proto, pc) = (caller.proto.clone(), caller.pc.checked_sub(1)?);
                let (Op::Call { func, .. } | Op::TailCall { func, .. }) = *proto.code.get(pc)?
                else {
                    return None;
                };
                self.describe_register(&proto, pc, func)
            }
            CallSite::Meta { event } => Some(("metamethod", event.as_bytes().to_vec())),
            CallSite::Native | CallSite::None => None,
        }
    }

    /// Names the register an error was about (`local 'a'`, `upvalue 'u'`, `global 'x'`, `field 'y'`, `method 'z'`),
    /// as Lua's messages do.
    fn name_error(&mut self, error: LuaError, proto: &Proto, pc: usize, register: u8) -> LuaError {
        let message = match &error {
            LuaError::Runtime(thrown) if !thrown.located => match thrown.value {
                Value::String(message) => message,
                _ => return error,
            },
            _ => return error,
        };
        let Some((kind, n)) = self.describe_register(proto, pc, register) else {
            return error;
        };
        if let Err(limit) = self.charge_fuel(1 + n.len() as u64 / 16) {
            return limit;
        }
        // "attempt to call a nil value" becomes "attempt to call global 'f' (a nil value)".
        let text = self.heap.string(message);
        let Some(article) = find(text, b" a ").or_else(|| find(text, b" an ")) else {
            return error;
        };
        let mut rewritten = text[..article].to_vec();
        rewritten.extend_from_slice(format!(" {kind} '").as_bytes());
        rewritten.extend_from_slice(crate::c_string_prefix(&n));
        rewritten.extend_from_slice(b"' (");
        rewritten.extend_from_slice(&text[article + 1..]);
        rewritten.push(b')');
        match self.heap.intern(&rewritten) {
            Ok(s) => LuaError::runtime(Value::String(s), Traceback::default(), false),
            Err(e) => e,
        }
    }

    /// Returns `count` values from `first` out of the running frame; true when that frame was a boundary.
    fn return_from_frame(&mut self, first: usize, count: usize) -> Result<bool, LuaError> {
        let frame = self.frames.pop().expect("a running frame");
        self.place_results(first, count, frame.slot, frame.want)?;
        if frame.catch.is_some() {
            self.stack[frame.slot - 1] = Value::Boolean(true);
            if self.top == frame.slot && frame.want < 0 {
                self.top = frame.slot;
            }
        }
        if frame.boundary {
            return Ok(true);
        }
        let caller = self.frames.last().expect("a caller");
        let caller_top = caller.base + usize::from(caller.proto.registers);
        if self.stack.len() < caller_top {
            self.set_stack(caller_top)?;
        }
        Ok(false)
    }

    // --- the collector ---------------------------------------------------------------------------------------
    //
    // Incremental mark and sweep. Marking traces a few gray values at each of the collector's points (between two
    // instructions), behind a write barrier; then the roots are traced once more and the sweep frees what is unmarked,
    // a few slots at each point. Every step pays fuel as it goes and stops when the fuel is gone, so the collector's
    // work never outruns the call's fuel; the next point goes on from there. Memory is recounted from what the sweep
    // kept, plus what was charged while it ran.

    /// `collectgarbage("collect")`: a whole collection at the next instruction.
    pub(crate) fn request_collection(&mut self) {
        self.gc_requested = true;
    }

    /// The collector's point between two instructions: a step of the collection under way, else the start of one
    /// when memory reached the threshold. A requested collection, a stress mode or memory past the cap run whole
    /// collections now. Fails when, after a collection, what is alive is still over the cap.
    #[inline]
    fn gc_point(&mut self) -> Result<(), LuaError> {
        if self.gc_requested || self.gc_stress == GcStress::Full {
            return self.collect();
        }
        if self.heap.phase == Phase::Idle {
            if self.heap.memory < self.gc_threshold {
                return Ok(());
            }
            self.gc_start()?;
        }
        // Past the cap, finish now rather than let allocation outrun the collection.
        if self.heap.memory > self.heap.cap {
            return self.gc_complete();
        }
        self.gc_advance(self.gc_stress.step())
    }

    /// A whole collection now: the one under way finished, then a new one from the start (the host's `collect`,
    /// `collectgarbage("collect")`).
    pub(crate) fn collect(&mut self) -> Result<(), LuaError> {
        self.gc_requested = false;
        self.gc_complete()?;
        self.gc_start()?;
        self.gc_complete()
    }

    /// Runs the collection under way to its end.
    fn gc_complete(&mut self) -> Result<(), LuaError> {
        // Each advance with no bound ends its phase (or runs out of fuel): marking, then sweeping.
        for _ in 0..2 {
            if self.heap.phase == Phase::Idle {
                return Ok(());
            }
            self.gc_advance(usize::MAX)?;
        }
        assert_eq!(
            self.heap.phase,
            Phase::Idle,
            "two unbounded advances end a collection"
        );
        Ok(())
    }

    /// Up to `budget` work of the collection under way: gray values traced, or slots swept.
    fn gc_advance(&mut self, budget: usize) -> Result<(), LuaError> {
        match self.heap.phase {
            Phase::Idle => Ok(()),
            Phase::Marking => {
                if self.trace_gray(budget)? {
                    self.gc_end_marking()
                } else {
                    Ok(())
                }
            }
            Phase::Sweeping => self.gc_sweep(budget),
        }
    }

    /// Every root, in one place: the running coroutine's stack and frames, what the host pinned, the registry. A
    /// suspended coroutine is not a root; it is reached through whatever holds it. Looking at the roots is work in
    /// their number, nil registers too: paid first, and only what can hold other values is grayed.
    fn push_roots(&mut self) -> Result<(), LuaError> {
        let names = &self.names;
        let fixed = [
            names.index,
            names.newindex,
            names.call,
            names.add,
            names.sub,
            names.mul,
            names.div,
            names.modulo,
            names.pow,
            names.unm,
            names.concat,
            names.len,
            names.eq,
            names.lt,
            names.le,
            names.tostring,
            names.metatable,
            names.name,
        ]
        .map(Value::String);
        let registry = [
            Some(Value::Table(self.globals)),
            Some(Value::Table(self.loaded)),
            self.string_meta.map(Value::Table),
            Some(Value::Thread(self.main)),
            Some(Value::Thread(self.current)),
        ];
        let roots = self.stack.len()
            + self.pinned.len()
            + 2 * self.frames.len()
            + fixed.len()
            + registry.len();
        self.charge_fuel(roots.div_ceil(GC_OBJECTS_PER_FUEL) as u64)?;
        crate::work::visit(crate::work::Work::Collector, roots);
        let gray = &mut self.heap.gray;
        let objects =
            |value: &&Value| !matches!(value, Value::Nil | Value::Boolean(_) | Value::Number(_));
        gray.extend(self.stack.iter().filter(objects));
        gray.extend(self.pinned.iter().filter(objects));
        Self::push_frames(&self.frames, gray);
        gray.extend(registry.into_iter().flatten());
        gray.extend(fixed);
        self.heap.count_gray();
        Ok(())
    }

    /// What a list of frames holds beyond its stack: the functions running, and the handlers of `xpcall`s waiting.
    fn push_frames(frames: &[Frame], gray: &mut crate::heap::Gray) {
        for frame in frames {
            gray.push(Value::Function(frame.function));
            if let Some(catch) = frame.catch {
                gray.push(catch.handler);
            }
        }
    }

    fn gc_start(&mut self) -> Result<(), LuaError> {
        assert_eq!(self.heap.phase, Phase::Idle, "one collection at a time");
        self.gc_epoch += 1;
        self.gc_protos = 0;
        self.heap.gray.clear();
        assert!(
            self.heap.partial.is_none(),
            "a collection ends with no object half traced"
        );
        self.heap.phase = Phase::Marking;
        let heap = &mut self.heap;
        heap.strings.begin_collection();
        heap.tables.begin_collection();
        heap.functions.begin_collection();
        heap.cells.begin_collection();
        heap.userdata.begin_collection();
        self.threads.begin_collection();
        self.push_roots()
    }

    /// Collect again when memory doubles (at least a megabyte later), never past the cap; at once under stress.
    pub(crate) fn next_threshold(&self, live: usize) -> usize {
        if self.gc_stress == GcStress::Off {
            (live * 2).max(live + MIN_THRESHOLD).min(self.heap.cap)
        } else {
            0
        }
    }

    /// Traces gray values until none is left (true) or `budget` work is done (false), paying fuel as it goes: no work
    /// is done that the fuel left does not pay for, and a large object is traced in pieces across calls.
    fn trace_gray(&mut self, budget: usize) -> Result<bool, LuaError> {
        let affordable = self.affordable_work();
        let mut traced = 0usize;
        let done = loop {
            let room = budget.min(affordable).saturating_sub(traced);
            if let Some(partial) = self.heap.partial {
                if room == 0 {
                    break false;
                }
                traced += self.trace_piece(partial, room.min(TRACE_PIECE));
                continue;
            }
            let Some(&value) = self.heap.gray.last() else {
                break true;
            };
            if self.is_traced(value) {
                // Grayed again, or a value that refers to nothing: what grayed it paid for it.
                self.heap.gray.pop();
                continue;
            }
            if room == 0 {
                break false;
            }
            self.heap.gray.pop();
            let work = self.trace(value);
            crate::work::visit(crate::work::Work::Collector, work);
            traced += work;
        };
        self.heap.count_gray();
        self.charge_fuel((traced / GC_OBJECTS_PER_FUEL) as u64)?;
        if !done && traced < budget {
            return Err(self.out_of_fuel());
        }
        Ok(done)
    }

    /// Whether tracing `value` would be no work: it refers to nothing, or it is marked already.
    fn is_traced(&self, value: Value) -> bool {
        let heap = &self.heap;
        match value {
            Value::Nil | Value::Boolean(_) | Value::Number(_) => true,
            Value::String(s) => heap.strings.is_marked(s.0),
            Value::Table(t) => heap.tables.is_marked(t.0),
            Value::Function(f) => heap.functions.is_marked(f.0),
            Value::Cell(c) => heap.cells.is_marked(c.0),
            Value::Userdata(u) => heap.userdata.is_marked(u.0),
            Value::Thread(t) => self.threads.is_marked(t.0),
        }
    }

    /// Traces up to `room` (at least one) units of the partly traced object; returns the work done. A string found
    /// is marked at once (it refers to nothing), so that a large table of strings does not fill the worklist.
    fn trace_piece(&mut self, partial: Partial, room: usize) -> usize {
        let Heap {
            tables,
            strings,
            gray,
            partial: left,
            ..
        } = &mut self.heap;
        let mut reached = |value: Value| match value {
            Value::Nil | Value::Boolean(_) | Value::Number(_) => {}
            Value::String(s) => {
                strings.mark(s.0);
            }
            object => gray.push(object),
        };
        let work = match partial {
            Partial::Table { id, entries, array } => {
                let table = tables.get(id.0);
                let entries = entries.min(table.entry_count());
                if array.is_none() && entries > 0 {
                    let from = entries.saturating_sub(room);
                    for index in (from..entries).rev() {
                        // A removed key is never looked at again: its key need not stay alive.
                        let (key, value) = table.entry(index);
                        if value != Value::Nil {
                            reached(key);
                            reached(value);
                        }
                    }
                    *left = Some(Partial::Table {
                        id,
                        entries: from,
                        array: None,
                    });
                    entries - from
                } else {
                    let end = array
                        .unwrap_or(table.array().len())
                        .min(table.array().len());
                    let from = end.saturating_sub(room);
                    table.array()[from..end]
                        .iter()
                        .for_each(|&value| reached(value));
                    *left = (from > 0).then_some(Partial::Table {
                        id,
                        entries: 0,
                        array: Some(from),
                    });
                    end - from
                }
            }
            Partial::Thread { id, stack } => {
                let thread = self.threads.get(id.0);
                let end = stack.min(thread.stack.len());
                let from = end.saturating_sub(room);
                thread.stack[from..end]
                    .iter()
                    .for_each(|&value| reached(value));
                *left = (from > 0).then_some(Partial::Thread { id, stack: from });
                end - from
            }
        };
        crate::work::visit(crate::work::Work::Collector, work);
        // An empty remainder is one unit: the piece that finds it so.
        work.max(1)
    }

    /// The collector work the fuel left pays for.
    fn affordable_work(&self) -> usize {
        self.heap.fuel.affordable(GC_OBJECTS_PER_FUEL)
    }

    fn out_of_fuel(&mut self) -> LuaError {
        self.heap.fuel.exhaust()
    }

    /// The end of marking: the roots once more (they changed while marking went on) and every gray value traced; then
    /// the sweep begins. Out of fuel on the way, marking stays unfinished, and the next point ends it.
    fn gc_end_marking(&mut self) -> Result<(), LuaError> {
        self.push_roots()?;
        let done = self.trace_gray(usize::MAX)?;
        assert!(done, "an unbounded trace ends with no gray value left");
        debug_assert!(
            self.heap.partial.is_none(),
            "an unbounded trace ends every partial object"
        );
        // The worklist is empty until the next collection: its storage goes back, and out of the count with the sweep.
        self.heap.gray.release();
        self.heap.phase = Phase::Sweeping;
        self.gc_arena = SweepArena::Strings;
        self.gc_kept = 0;
        self.gc_memory_at_sweep = self.heap.memory;
        Ok(())
    }

    /// Sweeps up to `budget` slots, arena after arena, each step paid before it is walked (4 slots a fuel, rounded
    /// down over the whole sweep step); ends each arena as soon as its last slot is swept (shrinking the string index
    /// when that is paid for too), and the sweep with the last. Out of fuel, it stops between two steps, in a state
    /// the next point resumes.
    fn gc_sweep(&mut self, budget: usize) -> Result<(), LuaError> {
        let mut walked = 0usize;
        loop {
            while self.gc_arena != SweepArena::Done && self.gc_arena_swept(&mut walked) {
                self.gc_arena = self.gc_arena.next();
            }
            if self.gc_arena == SweepArena::Done {
                return self.gc_end_sweep();
            }
            if walked >= budget {
                return Ok(());
            }
            let heap = &self.heap;
            let unswept = match self.gc_arena {
                SweepArena::Strings => heap.strings.unswept(),
                SweepArena::Tables => heap.tables.unswept(),
                SweepArena::Functions => heap.functions.unswept(),
                SweepArena::Cells => heap.cells.unswept(),
                SweepArena::Userdata => heap.userdata.unswept(),
                SweepArena::Threads => self.threads.unswept(),
                SweepArena::Done => 0,
            };
            let step = (budget - walked)
                .min(unswept)
                .min(self.payable_work(walked));
            if step == 0 {
                return Err(self.out_of_fuel());
            }
            self.pay_work(&mut walked, step);
            let heap = &mut self.heap;
            let (kept, slots) = match self.gc_arena {
                SweepArena::Strings => heap.sweep_strings(step),
                SweepArena::Tables => heap.tables.sweep(step, Table::bytes, |_, _| ()),
                SweepArena::Functions => heap.functions.sweep(step, function_bytes, |_, _| ()),
                SweepArena::Cells => heap
                    .cells
                    .sweep(step, |_| crate::heap::CELL_SIZE, |_, _| ()),
                SweepArena::Userdata => {
                    heap.userdata
                        .sweep(step, |u| crate::heap::USERDATA_SIZE + u.bytes, |_, _| ())
                }
                SweepArena::Threads => self.threads.sweep(step, ThreadState::bytes, |_, _| ()),
                SweepArena::Done => (0, 0),
            };
            debug_assert_eq!(slots, step, "a paid step walks every slot it paid for");
            crate::work::visit(crate::work::Work::Collector, slots);
            self.gc_kept += kept;
        }
    }

    /// The collector's units of work (4 a fuel, rounded down over a run of them) the fuel left pays for, after a run
    /// that already did `done` units and paid for them.
    fn payable_work(&self, done: usize) -> usize {
        let paid = done / GC_OBJECTS_PER_FUEL;
        let left = usize::try_from(self.heap.fuel.left).unwrap_or(usize::MAX);
        left.saturating_add(paid)
            .saturating_mul(GC_OBJECTS_PER_FUEL)
            .saturating_add(GC_OBJECTS_PER_FUEL - 1)
            .saturating_sub(done)
    }

    /// Pays for `units` more of a run that did `done` units: never fails for `units` up to `payable_work(done)`.
    fn pay_work(&mut self, done: &mut usize, units: usize) {
        let owed = (*done + units) / GC_OBJECTS_PER_FUEL - *done / GC_OBJECTS_PER_FUEL;
        self.heap.fuel.left -= owed as u64;
        *done += units;
    }

    /// When the arena being swept is done: counts its storage (and what goes with it), ends its sweep and returns
    /// true; false while it has slots left. Shrinking the string index walks it: done, paid before as part of the run
    /// `walked` counts, when the fuel left pays for it, else left for another collection.
    fn gc_arena_swept(&mut self, walked: &mut usize) -> bool {
        if self.gc_arena == SweepArena::Strings && self.heap.strings.is_swept() {
            let size = self.heap.intern_size();
            if self.heap.shrinkable_intern() && size <= self.payable_work(*walked) {
                self.pay_work(walked, size);
                crate::work::visit(crate::work::Work::Collector, size);
                self.heap.shrink_intern();
            }
        }
        let heap = &mut self.heap;
        let storage = match self.gc_arena {
            SweepArena::Strings if heap.strings.is_swept() => {
                heap.strings.end_sweep(crate::heap::STRING_OVERHEAD)
                    + heap.intern_size() * crate::heap::INTERN_ENTRY
            }
            SweepArena::Tables if heap.tables.is_swept() => {
                heap.tables.end_sweep(crate::heap::TABLE_SIZE)
            }
            SweepArena::Functions if heap.functions.is_swept() => {
                heap.functions.end_sweep(crate::heap::FUNCTION_SIZE)
            }
            SweepArena::Cells if heap.cells.is_swept() => {
                heap.cells.end_sweep(crate::heap::CELL_SIZE)
            }
            SweepArena::Userdata if heap.userdata.is_swept() => {
                heap.userdata.end_sweep(crate::heap::USERDATA_SIZE)
            }
            // The running coroutine's stack is out of its slot: counted with the arena, as the others are.
            SweepArena::Threads if self.threads.is_swept() => {
                self.threads.end_sweep(THREAD_BYTES)
                    + self.heap.gray.bytes()
                    + self.pinned.capacity() * SLOT_BYTES
                    + self.stack.capacity() * SLOT_BYTES
                    + self.frames.capacity() * FRAME_BYTES
            }
            _ => return false,
        };
        self.gc_kept += storage;
        true
    }

    /// The end of the sweep: memory is what was kept, plus what was charged since the sweep began (an object made
    /// in an arena the sweep had yet to reach is counted twice, which errs on the safe side until the next count).
    fn gc_end_sweep(&mut self) -> Result<(), LuaError> {
        let charged_since = self.heap.memory - self.gc_memory_at_sweep;
        let live = self.gc_kept + self.gc_protos + charged_since;
        self.heap.memory = live;
        self.heap.phase = Phase::Idle;
        self.gc_threshold = self.next_threshold(live);
        if live > self.heap.cap {
            return Err(LuaError::Limit(Limit::Memory));
        }
        Ok(())
    }

    /// Marks one value and grays what it refers to; returns the work it was (0 for a value already marked).
    fn trace(&mut self, value: Value) -> usize {
        let heap = &mut self.heap;
        match value {
            Value::String(s) => usize::from(heap.strings.mark(s.0)),
            Value::Table(t) => {
                if !heap.tables.mark(t.0) {
                    return 0;
                }
                let table = heap.tables.get(t.0);
                if let Some(mt) = table.metatable {
                    heap.gray.push(Value::Table(mt));
                }
                self.heap.partial = Some(Partial::Table {
                    id: t,
                    entries: table.entry_count(),
                    array: None,
                });
                1
            }
            Value::Function(f) => {
                if !heap.functions.mark(f.0) {
                    return 0;
                }
                match heap.functions.get(f.0) {
                    Function::Lua {
                        proto,
                        upvalues,
                        env,
                    } => {
                        let (proto, cells, env) = (proto.clone(), upvalues.clone(), *env);
                        heap.gray.extend(cells.iter().map(|c| Value::Cell(*c)));
                        heap.gray.push(Value::Table(env));
                        1 + cells.len() + self.trace_proto(proto)
                    }
                    Function::Native { upvalues, .. } => {
                        let upvalues = upvalues.to_vec();
                        let work = 1 + upvalues.len();
                        heap.gray.extend(upvalues);
                        work
                    }
                }
            }
            Value::Cell(c) => {
                if !heap.cells.mark(c.0) {
                    return 0;
                }
                let inner = *heap.cells.get(c.0);
                heap.gray.push(inner);
                1
            }
            Value::Userdata(u) => {
                if !heap.userdata.mark(u.0) {
                    return 0;
                }
                if let Some(mt) = heap.userdata.get(u.0).metatable {
                    heap.gray.push(Value::Table(mt));
                }
                1
            }
            Value::Thread(t) => {
                if !self.threads.mark(t.0) {
                    return 0;
                }
                // Frames are bounded by the call depth; the stack, as large as the frames' registers, goes in pieces.
                // The coroutine that resumed this one needs no edge: the call that resumed it holds this coroutine
                // (or the function `wrap` made) in its own stack, which is a root or reached in turn from main's.
                let thread = self.threads.get(t.0);
                Self::push_frames(&thread.frames, &mut self.heap.gray);
                self.heap.partial = Some(Partial::Thread {
                    id: t,
                    stack: thread.stack.len(),
                });
                1 + thread.frames.len()
            }
            Value::Nil | Value::Boolean(_) | Value::Number(_) => 0,
        }
    }

    /// Grays a function's constants and its nested functions' (once per collection); returns the work.
    fn trace_proto(&mut self, proto: Rc<Proto>) -> usize {
        let epoch = self.gc_epoch;
        let mut protos = vec![proto];
        let mut work = 0;
        while let Some(proto) = protos.pop() {
            if proto.visited.replace(epoch) == epoch {
                continue;
            }
            self.gc_protos += proto.bytes();
            self.heap.gray.extend_from_slice(&proto.constants);
            self.heap.gray.push(Value::String(proto.source));
            protos.extend(proto.protos.iter().cloned());
            work += 1 + proto.constants.len();
        }
        work
    }

    // --- coroutines ------------------------------------------------------------------------------------------

    pub(crate) fn new_thread(&mut self, function: Value) -> Result<ThreadId, LuaError> {
        let ceiling = self.heap.ceiling();
        self.threads.reserve(
            &mut self.heap.memory,
            ceiling,
            THREAD_BYTES,
            THREAD_BYTES,
            &mut self.heap.fuel,
        )?;
        let mut thread = ThreadState::new();
        thread.stack.push(function);
        self.heap.barrier(function);
        Ok(ThreadId(self.threads.insert(thread)))
    }

    /// Makes `to` the running thread, storing the current one's stack.
    fn switch(&mut self, to: ThreadId) {
        let from = self.current;
        let thread = self.threads.get_mut(from.0);
        std::mem::swap(&mut thread.stack, &mut self.stack);
        std::mem::swap(&mut thread.frames, &mut self.frames);
        std::mem::swap(&mut thread.charged, &mut self.stack_charged);
        thread.top = self.top;
        // A coroutine traced before it ran has new values on its stack now: it is traced again (and that is paid as
        // tracing is), rather than its whole stack copied here at every switch.
        if self.heap.phase == Phase::Marking {
            self.threads.unmark(from.0);
            self.heap.gray.push(Value::Thread(from));
            self.heap.count_gray();
            // A half-traced coroutine needs nothing more: the one stopping is traced again whole, and the one starting
            // leaves an empty stack in its slot (its own is the running stack, a root), where the trace ends.
        }
        let thread = self.threads.get_mut(to.0);
        std::mem::swap(&mut thread.stack, &mut self.stack);
        std::mem::swap(&mut thread.frames, &mut self.frames);
        // Each coroutine's stack is counted on its own: what one grows is charged however large another's is.
        std::mem::swap(&mut thread.charged, &mut self.stack_charged);
        self.top = thread.top;
        self.current = to;
    }

    /// Resumes `co` with `args`; Ok(Ok(values)) when it yielded or returned, Ok(Err(error value)) when it failed.
    pub(crate) fn resume(
        &mut self,
        co: ThreadId,
        args: Vec<Value>,
    ) -> Result<Result<Vec<Value>, Value>, LuaError> {
        let status = self.threads.get(co.0).status;
        if status != ThreadStatus::Fresh && status != ThreadStatus::Suspended {
            let message = match status {
                ThreadStatus::Dead => "cannot resume dead coroutine",
                ThreadStatus::Running => "cannot resume running coroutine",
                _ => "cannot resume non-suspended coroutine",
            };
            return Ok(Err(self.new_string(message.as_bytes())?));
        }
        let from = self.current;
        self.threads.get_mut(from.0).status = ThreadStatus::Normal;
        self.switch(co);
        let depth = self.native_depth;
        {
            let thread = self.threads.get_mut(co.0);
            thread.status = ThreadStatus::Running;
            thread.depth = depth;
        }
        let outcome = self.run_coroutine(status, args);
        self.finish_resume(co, from, outcome)
    }

    // Completion runs after nested execution, keeping its copy/error temporaries out of each retained driver.
    #[inline(never)]
    fn finish_resume(
        &mut self,
        co: ThreadId,
        from: ThreadId,
        outcome: Result<Exec, LuaError>,
    ) -> Result<Result<Vec<Value>, Value>, LuaError> {
        let (results, status) = match outcome {
            Ok(Exec::Yielded) => {
                let pending = self
                    .threads
                    .get(co.0)
                    .pending
                    .expect("a yield leaves its values");
                let values = self.stack[pending.values..].to_vec();
                self.stack.truncate(pending.values);
                (Ok(values), ThreadStatus::Suspended)
            }
            Ok(Exec::Returned) => {
                let values = self.stack[..self.top.min(self.stack.len())].to_vec();
                self.stack.clear();
                self.frames.clear();
                (Ok(values), ThreadStatus::Dead)
            }
            Err(LuaError::Runtime(thrown)) => {
                self.frames.clear();
                (Err(thrown.value), ThreadStatus::Dead)
            }
            Err(limit @ LuaError::Limit(_)) => {
                self.frames.clear();
                self.threads.get_mut(co.0).status = ThreadStatus::Dead;
                self.switch(from);
                self.threads.get_mut(from.0).status = ThreadStatus::Running;
                return Err(limit);
            }
        };
        self.threads.get_mut(co.0).status = status;
        self.switch(from);
        self.threads.get_mut(from.0).status = ThreadStatus::Running;
        Ok(results)
    }

    /// Runs the coroutine just switched to. Its loop nests in the native that resumes it (`coroutine.resume`, a
    /// `wrap` function), whose call counted the level: a resume is one native level, as a metamethod is.
    fn run_coroutine(&mut self, status: ThreadStatus, args: Vec<Value>) -> Result<Exec, LuaError> {
        // The coroutine runs its own instructions: no native of this thread is running in it.
        let caller = std::mem::replace(&mut self.call_site, CallSite::None);
        let result = (|| {
            if let Some((function, count)) = self.prepare_coroutine(status, args)? {
                let produced = self.call_native(function, 0, count, CallSite::Native)?;
                if self.yielding {
                    self.yielding = false;
                    return Err(self.error("attempt to yield across a C-call boundary"));
                }
                self.top = produced;
                return Ok(Exec::Returned);
            }
            // A coroutine's only boundary is its first frame: a yield across any other is refused.
            self.execute(0)
        })();
        self.call_site = caller;
        result.map_err(|e| self.locate(e, None))
    }

    // Setup owns argument and frame temporaries only until execution begins.
    #[inline(never)]
    fn prepare_coroutine(
        &mut self,
        status: ThreadStatus,
        args: Vec<Value>,
    ) -> Result<Option<(FuncId, usize)>, LuaError> {
        if status == ThreadStatus::Fresh {
            for arg in &args {
                self.push(*arg)?;
            }
            let count = args.len();
            let (function, count) = self.callable(0, count)?;
            if let Function::Native { .. } = self.heap.functions.get(function.0) {
                return Ok(Some((function, count)));
            }
            self.push_frame(function, 0, count, -1, true)?;
        } else {
            let pending = self
                .threads
                .get(self.current.0)
                .pending
                .expect("a suspended coroutine has a pending yield");
            self.stack.truncate(pending.slot);
            if pending.protected {
                self.push(Value::Boolean(true))?;
            }
            for arg in &args {
                self.push(*arg)?;
            }
            self.place_results(
                pending.slot,
                args.len() + usize::from(pending.protected),
                pending.slot,
                pending.want,
            )?;
            let frame = self.frames.last().expect("a suspended frame");
            let frame_top = frame.base + usize::from(frame.proto.registers);
            if self.stack.len() < frame_top {
                self.set_stack(frame_top)?;
            }
        }
        Ok(None)
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}
