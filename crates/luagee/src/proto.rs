//! Compiled functions: register bytecode, constants and line numbers.

use std::cell::Cell;
use std::rc::Rc;

use crate::value::{StrId, Value};

/// A register or a constant operand: below 256 a register, from 256 the constant `index - 256`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Rk(pub(crate) u16);

impl Rk {
    pub(crate) const CONSTANT: u16 = 256;
    pub(crate) const MAX_CONSTANT: u32 = (u16::MAX - Rk::CONSTANT) as u32;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ArithOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CompareOp {
    Eq,
    Lt,
    Le,
}

/// One instruction. `a` is the destination register unless said otherwise.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Op {
    Move {
        a: u8,
        b: u8,
    },
    LoadK {
        a: u8,
        k: u32,
    },
    /// `R[a] = value`, then skip the next instruction when `skip`.
    LoadBool {
        a: u8,
        value: bool,
        skip: bool,
    },
    /// `R[a..=a + count] = nil`.
    LoadNil {
        a: u8,
        count: u8,
    },
    /// Wraps `R[a]` into a new cell: the register now holds a captured local.
    NewCell {
        a: u8,
    },
    /// `R[a] = value of the cell in R[cell]`.
    GetCell {
        a: u8,
        cell: u8,
    },
    /// `value of the cell in R[cell] = R[b]`.
    SetCell {
        cell: u8,
        b: u8,
    },
    GetUpvalue {
        a: u8,
        upvalue: u8,
    },
    SetUpvalue {
        upvalue: u8,
        b: u8,
    },
    /// `R[a] = env[K[k]]`.
    GetGlobal {
        a: u8,
        k: u32,
    },
    /// `env[K[k]] = R[b]`.
    SetGlobal {
        b: u8,
        k: u32,
    },
    GetTable {
        a: u8,
        table: u8,
        key: Rk,
    },
    SetTable {
        table: u8,
        key: Rk,
        value: Rk,
    },
    NewTable {
        a: u8,
        array: u16,
        hash: u16,
    },
    /// `R[a + 1] = R[object]; R[a] = R[object][key]`.
    SelfMethod {
        a: u8,
        object: u8,
        key: Rk,
    },
    Arith {
        op: ArithOp,
        a: u8,
        b: Rk,
        c: Rk,
    },
    Neg {
        a: u8,
        b: u8,
    },
    Not {
        a: u8,
        b: u8,
    },
    Len {
        a: u8,
        b: u8,
    },
    /// `R[a] = R[first] .. ... .. R[last]`.
    Concat {
        a: u8,
        first: u8,
        last: u8,
    },
    Jump {
        offset: i32,
    },
    /// The next instruction (a jump) runs exactly when `(b op c) == expect`.
    Compare {
        op: CompareOp,
        b: Rk,
        c: Rk,
        expect: bool,
    },
    /// The next instruction (a jump) runs exactly when `R[a]`'s truth is `expect`.
    Test {
        a: u8,
        expect: bool,
    },
    /// Calls `R[func]` with `args - 1` arguments (0: up to the top) for `results - 1` results (0: all, setting the
    /// top).
    Call {
        func: u8,
        args: u8,
        results: u8,
    },
    TailCall {
        func: u8,
        args: u8,
    },
    /// Returns `count - 1` values from `R[first]` (0: up to the top).
    Return {
        first: u8,
        count: u8,
    },
    ForPrep {
        a: u8,
        offset: i32,
    },
    ForLoop {
        a: u8,
        offset: i32,
    },
    /// The generic `for`'s test after its call: if `R[a + 3]` is not nil, `R[a + 2] = R[a + 3]` and jump.
    ForEachLoop {
        a: u8,
        offset: i32,
    },
    /// `R[table][start + i] = R[table + 1 + i]` for `count - 1` values (0: up to the top).
    SetList {
        table: u8,
        count: u8,
        start: u32,
    },
    Closure {
        a: u8,
        proto: u32,
    },
    /// `R[a..] = ...`, `count - 1` of them (0: all, setting the top).
    VarArg {
        a: u8,
        count: u8,
    },
}

/// Where a new closure finds an upvalue: a cell in the creating function's register, or the creating function's own
/// upvalue.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Capture {
    Register(u8),
    Upvalue(u8),
}

/// A local variable's name and where it lives, for error messages.
#[derive(Debug)]
pub(crate) struct LocalInfo {
    pub(crate) name: Box<[u8]>,
    pub(crate) register: u8,
    /// The instructions it is active for: `start..end`.
    pub(crate) start: usize,
    pub(crate) end: usize,
}

/// A compiled function.
#[derive(Debug)]
pub(crate) struct Proto {
    pub(crate) code: Vec<Op>,
    pub(crate) lines: Vec<u32>,
    /// Numbers, strings, booleans and nil.
    pub(crate) constants: Vec<Value>,
    pub(crate) protos: Vec<Rc<Proto>>,
    pub(crate) captures: Vec<Capture>,
    pub(crate) locals: Vec<LocalInfo>,
    pub(crate) upvalue_names: Vec<Box<[u8]>>,
    pub(crate) params: u8,
    pub(crate) vararg: bool,
    pub(crate) registers: u8,
    /// The chunk's name, as error positions print it.
    pub(crate) chunk: Rc<str>,
    /// The chunk's name as it was given (`@file`, `=name`, or the source itself): `debug.getinfo`'s `source`. The
    /// collector keeps it with the function.
    pub(crate) source: StrId,
    pub(crate) line: u32,
    /// The collector's mark: the collection that last visited this function's constants.
    pub(crate) visited: Cell<u64>,
}

impl Proto {
    /// The bytes this function's code and constants are counted as.
    pub(crate) fn bytes(&self) -> usize {
        128 + self.code.len() * 16 + self.constants.len() * 16 + self.captures.len() * 2
    }
}
