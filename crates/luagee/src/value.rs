//! What a Lua value is: a number, a boolean, nil, or a handle to an object on the interpreter's heap.

use std::sync::atomic::{AtomicU64, Ordering};

use crate::{Limit, LuaError};

/// Interpreter ownership is host-only metadata; scripts hash and display only local slot indexes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct Handle {
    pub(crate) local: u64,
    pub(crate) owner: u64,
}

impl Handle {
    pub(crate) fn index(self) -> u32 {
        self.local as u32
    }
    pub(crate) fn generation(self) -> u32 {
        (self.local >> 32) as u32
    }
}

pub(crate) const HANDLE_BYTES: usize = 16;
pub(crate) const VALUE_BYTES: usize = 24;
static NEXT_OWNER: AtomicU64 = AtomicU64::new(1);

pub(crate) fn next_owner() -> Result<u64, LuaError> {
    NEXT_OWNER
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |owner| {
            owner.checked_add(1)
        })
        .map_err(|_| LuaError::Limit(Limit::Memory))
}

/// A string on the heap. Strings are interned: two strings are equal exactly when their handles are.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct StrId(pub(crate) Handle);

/// A table on the heap.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TableId(pub(crate) Handle);

/// A function (Lua or native) on the heap.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FuncId(pub(crate) Handle);

/// A userdata on the heap: a host value scripts see only through its metatable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct UserdataId(pub(crate) Handle);

/// A coroutine on the heap.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ThreadId(pub(crate) Handle);

/// A heap cell holding a local variable that a closure captured; it lives only in registers, never in a value a
/// script or the host sees.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CellId(pub(crate) Handle);

/// A Lua value.
///
/// Handles contain interpreter ownership, an index and a generation; a handle the host keeps beyond the call that gave it must be
/// pinned ([`Lua::pin`](crate::Lua::pin)), or the collector may take its object.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Value {
    /// `nil`.
    #[default]
    Nil,
    /// `true` or `false`.
    Boolean(bool),
    /// A double.
    Number(f64),
    /// A string.
    String(StrId),
    /// A table.
    Table(TableId),
    /// A function.
    Function(FuncId),
    /// A userdata.
    Userdata(UserdataId),
    /// A coroutine.
    Thread(ThreadId),
    /// A captured local's cell, only ever in a register the compiler reserved for it: scripts never see one.
    Cell(CellId),
}

impl Value {
    /// Lua's truth: everything but `nil` and `false`.
    #[must_use]
    #[inline]
    pub fn is_truthy(self) -> bool {
        !matches!(self, Value::Nil | Value::Boolean(false))
    }

    /// The name `type` gives.
    #[must_use]
    pub fn type_name(self) -> &'static str {
        match self {
            Value::Nil => "nil",
            Value::Boolean(_) => "boolean",
            Value::Number(_) => "number",
            Value::String(_) => "string",
            Value::Table(_) => "table",
            Value::Function(_) => "function",
            Value::Userdata(_) => "userdata",
            Value::Thread(_) => "thread",
            Value::Cell(_) => "cell",
        }
    }
}

const _: () = assert!(std::mem::size_of::<Handle>() == HANDLE_BYTES);
const _: () = assert!(std::mem::size_of::<Value>() <= VALUE_BYTES);
