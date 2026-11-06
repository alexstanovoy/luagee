//! The interpreter's heap: arenas of objects addressed by index, interned strings, and the memory account.
//!
//! Memory is counted by the interpreter itself (a string's bytes, a table's slots, a stack's values), never asked of
//! the allocator, so the cap means the same on every platform. The collector (in `vm`) marks from the roots and
//! sweeps these arenas; a freed slot's index is reused.

use std::any::Any;
use std::rc::Rc;

use crate::proto::Proto;
use crate::table::Table;
use crate::value::{CellId, FuncId, StrId, TableId, UserdataId, Value};
use crate::vm::{Args, LuaError, Vm};

/// SipHash-1-3, keyed: table keys hash with a key the script never sees, so it cannot pick keys that collide.
#[derive(Clone, Copy, Debug)]
pub(crate) struct KeyHasher {
    k0: u64,
    k1: u64,
}

macro_rules! sip_round {
    ($v0:ident, $v1:ident, $v2:ident, $v3:ident) => {
        $v0 = $v0.wrapping_add($v1);
        $v1 = $v1.rotate_left(13);
        $v1 ^= $v0;
        $v0 = $v0.rotate_left(32);
        $v2 = $v2.wrapping_add($v3);
        $v3 = $v3.rotate_left(16);
        $v3 ^= $v2;
        $v0 = $v0.wrapping_add($v3);
        $v3 = $v3.rotate_left(21);
        $v3 ^= $v0;
        $v2 = $v2.wrapping_add($v1);
        $v1 = $v1.rotate_left(17);
        $v1 ^= $v2;
        $v2 = $v2.rotate_left(32);
    };
}

impl KeyHasher {
    pub(crate) fn new(seed: u64) -> KeyHasher {
        KeyHasher {
            k0: seed,
            k1: seed.rotate_left(32) ^ 0x9e37_79b9_7f4a_7c15,
        }
    }

    /// The hash of a byte string.
    pub(crate) fn bytes(&self, data: &[u8]) -> u64 {
        let mut v0 = self.k0 ^ 0x736f_6d65_7073_6575;
        let mut v1 = self.k1 ^ 0x646f_7261_6e64_6f6d;
        let mut v2 = self.k0 ^ 0x6c79_6765_6e65_7261;
        let mut v3 = self.k1 ^ 0x7465_6462_7974_6573;
        let (chunks, remainder) = data.as_chunks::<8>();
        for chunk in chunks {
            let m = u64::from_le_bytes(*chunk);
            v3 ^= m;
            sip_round!(v0, v1, v2, v3);
            v0 ^= m;
        }
        let mut last = (data.len() as u64) << 56;
        for (i, &b) in remainder.iter().enumerate() {
            last |= u64::from(b) << (8 * i);
        }
        v3 ^= last;
        sip_round!(v0, v1, v2, v3);
        v0 ^= last;
        v2 ^= 0xff;
        sip_round!(v0, v1, v2, v3);
        sip_round!(v0, v1, v2, v3);
        sip_round!(v0, v1, v2, v3);
        v0 ^ v1 ^ v2 ^ v3
    }

    fn word(&self, tag: u8, word: u64) -> u64 {
        let mut data = [0u8; 9];
        data[0] = tag;
        data[1..].copy_from_slice(&word.to_le_bytes());
        self.bytes(&data)
    }

    /// The hash of a number key; 0 and -0 hash alike.
    #[inline]
    pub(crate) fn number(&self, n: f64) -> u64 {
        let n = if n == 0.0 { 0.0 } else { n };
        self.word(1, n.to_bits())
    }
}

/// An arena slot.
#[derive(Debug)]
struct Slot<T> {
    object: Option<T>,
    marked: bool,
}

/// Objects of one kind, addressed by index.
#[derive(Debug)]
pub(crate) struct Arena<T> {
    slots: Vec<Slot<T>>,
    free: Vec<u32>,
}

impl<T> Default for Arena<T> {
    fn default() -> Self {
        Arena {
            slots: Vec::new(),
            free: Vec::new(),
        }
    }
}

impl<T> Arena<T> {
    pub(crate) fn insert(&mut self, object: T) -> u32 {
        match self.free.pop() {
            Some(i) => {
                self.slots[i as usize] = Slot {
                    object: Some(object),
                    marked: false,
                };
                i
            }
            None => {
                self.slots.push(Slot {
                    object: Some(object),
                    marked: false,
                });
                u32::try_from(self.slots.len() - 1).expect("an arena holds fewer than 2^32 objects")
            }
        }
    }

    #[inline]
    pub(crate) fn get(&self, i: u32) -> &T {
        self.slots[i as usize]
            .object
            .as_ref()
            .expect("a live object")
    }

    #[inline]
    pub(crate) fn get_mut(&mut self, i: u32) -> &mut T {
        self.slots[i as usize]
            .object
            .as_mut()
            .expect("a live object")
    }

    /// Marks an object; true when it was not marked yet.
    pub(crate) fn mark(&mut self, i: u32) -> bool {
        let slot = &mut self.slots[i as usize];
        !std::mem::replace(&mut slot.marked, true)
    }

    /// Frees every unmarked object and clears the marks; calls `freed` for each freed index.
    pub(crate) fn sweep(&mut self, mut freed: impl FnMut(u32, &T)) {
        for (i, slot) in self.slots.iter_mut().enumerate() {
            if slot.marked {
                slot.marked = false;
            } else if let Some(object) = slot.object.take() {
                freed(i as u32, &object);
                self.free.push(i as u32);
            }
        }
    }
}

/// An interned string.
#[derive(Debug)]
pub(crate) struct LuaString {
    /// Shared, so a library function can hold a string's bytes while the heap changes, without copying them.
    pub(crate) bytes: Rc<[u8]>,
    pub(crate) hash: u64,
}

/// A host function's body.
pub type HostFn = dyn Fn(&mut Vm, Args) -> Result<usize, LuaError>;

/// A native function: a builtin or the host's.
#[derive(Clone)]
pub(crate) enum NativeFn {
    Builtin(fn(&mut Vm, Args) -> Result<usize, LuaError>),
    Host(Rc<HostFn>),
}

impl std::fmt::Debug for NativeFn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("native")
    }
}

/// A function object.
#[derive(Debug)]
pub(crate) enum Function {
    Lua {
        proto: Rc<Proto>,
        upvalues: Rc<[CellId]>,
        env: TableId,
    },
    Native {
        function: NativeFn,
        upvalues: Box<[Value]>,
        name: &'static str,
    },
}

/// A userdata object.
pub(crate) struct Userdata {
    pub(crate) data: Box<dyn Any>,
    pub(crate) metatable: Option<TableId>,
}

impl std::fmt::Debug for Userdata {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("userdata")
    }
}

/// The heap.
pub(crate) struct Heap {
    pub(crate) strings: Arena<LuaString>,
    /// Open addressing over `strings` by content: 0 is empty, else the id + 1.
    intern: Vec<u32>,
    interned: usize,
    pub(crate) tables: Arena<Table>,
    pub(crate) functions: Arena<Function>,
    pub(crate) userdata: Arena<Userdata>,
    pub(crate) cells: Arena<Value>,
    pub(crate) hasher: KeyHasher,
    /// The bytes held, as counted.
    pub(crate) memory: usize,
    /// The cap on `memory`.
    pub(crate) cap: usize,
}

/// The counted size of a string object beyond its bytes.
pub(crate) const STRING_OVERHEAD: usize = 32;
/// The counted size of a function object.
pub(crate) const FUNCTION_SIZE: usize = 64;
/// The counted size of a cell.
pub(crate) const CELL_SIZE: usize = 24;
/// The counted size of a userdata object (its host data is the host's).
pub(crate) const USERDATA_SIZE: usize = 48;

impl Heap {
    pub(crate) fn new(cap: usize, seed: u64) -> Heap {
        Heap {
            strings: Arena::default(),
            intern: vec![0; 1024],
            interned: 0,
            tables: Arena::default(),
            functions: Arena::default(),
            userdata: Arena::default(),
            cells: Arena::default(),
            hasher: KeyHasher::new(seed),
            memory: 0,
            cap,
        }
    }

    /// Counts `bytes` more, failing when that crosses the cap.
    #[inline]
    pub(crate) fn charge(&mut self, bytes: usize) -> Result<(), LuaError> {
        let total = self.memory.saturating_add(bytes);
        if total > self.cap {
            return Err(LuaError::Limit(crate::Limit::Memory));
        }
        self.memory = total;
        Ok(())
    }

    /// The interned string with these bytes, made if needed.
    pub(crate) fn intern(&mut self, bytes: &[u8]) -> Result<StrId, LuaError> {
        let hash = self.hasher.bytes(bytes);
        let mask = self.intern.len() - 1;
        let mut slot = hash as usize & mask;
        while self.intern[slot] != 0 {
            let id = self.intern[slot] - 1;
            let s = self.strings.get(id);
            if s.hash == hash && *s.bytes == *bytes {
                return Ok(StrId(id));
            }
            slot = (slot + 1) & mask;
        }
        self.charge(STRING_OVERHEAD + bytes.len())?;
        let id = self.strings.insert(LuaString {
            bytes: bytes.into(),
            hash,
        });
        self.intern[slot] = id + 1;
        self.interned += 1;
        if self.interned * 2 > self.intern.len() {
            self.rebuild_intern(self.intern.len() * 2);
        }
        Ok(StrId(id))
    }

    /// Rebuilds the intern index (after a sweep freed strings, or to grow it).
    pub(crate) fn rebuild_intern(&mut self, size: usize) {
        let live: Vec<(u32, u64)> = self
            .strings
            .slots
            .iter()
            .enumerate()
            .filter_map(|(i, s)| s.object.as_ref().map(|o| (i as u32, o.hash)))
            .collect();
        let size = size.max((live.len() * 2 + 2).next_power_of_two()).max(1024);
        self.intern.clear();
        self.intern.resize(size, 0);
        let mask = size - 1;
        for (id, hash) in &live {
            let mut slot = *hash as usize & mask;
            while self.intern[slot] != 0 {
                slot = (slot + 1) & mask;
            }
            self.intern[slot] = id + 1;
        }
        self.interned = live.len();
    }

    pub(crate) fn intern_index_size(&self) -> usize {
        self.intern.len()
    }

    #[inline]
    pub(crate) fn string(&self, id: StrId) -> &[u8] {
        &self.strings.get(id.0).bytes
    }

    /// A string's bytes, shared.
    pub(crate) fn string_rc(&self, id: StrId) -> Rc<[u8]> {
        self.strings.get(id.0).bytes.clone()
    }

    #[inline]
    pub(crate) fn table(&self, id: TableId) -> &Table {
        self.tables.get(id.0)
    }

    #[inline]
    pub(crate) fn table_mut(&mut self, id: TableId) -> &mut Table {
        self.tables.get_mut(id.0)
    }

    pub(crate) fn new_table(&mut self, array: usize, hash: usize) -> Result<TableId, LuaError> {
        let table = Table::with_capacity(array, hash);
        self.charge(table.bytes())?;
        Ok(TableId(self.tables.insert(table)))
    }

    pub(crate) fn new_function(&mut self, function: Function) -> Result<FuncId, LuaError> {
        let size = FUNCTION_SIZE
            + match &function {
                Function::Lua { upvalues, .. } => upvalues.len() * 8,
                Function::Native { upvalues, .. } => upvalues.len() * 16,
            };
        self.charge(size)?;
        Ok(FuncId(self.functions.insert(function)))
    }

    pub(crate) fn new_cell(&mut self, value: Value) -> Result<CellId, LuaError> {
        self.charge(CELL_SIZE)?;
        Ok(CellId(self.cells.insert(value)))
    }

    pub(crate) fn new_userdata(
        &mut self,
        data: Box<dyn Any>,
        metatable: Option<TableId>,
    ) -> Result<UserdataId, LuaError> {
        self.charge(USERDATA_SIZE)?;
        Ok(UserdataId(
            self.userdata.insert(Userdata { data, metatable }),
        ))
    }

    /// The hash of a table key (not nil, not NaN).
    #[inline]
    pub(crate) fn key_hash(&self, key: Value) -> u64 {
        match key {
            Value::String(s) => self.strings.get(s.0).hash,
            Value::Number(n) => self.hasher.number(n),
            Value::Boolean(b) => self.hasher_word(2, u64::from(b)),
            Value::Table(t) => self.hasher_word(3, u64::from(t.0)),
            Value::Function(f) => self.hasher_word(4, u64::from(f.0)),
            Value::Userdata(u) => self.hasher_word(5, u64::from(u.0)),
            Value::Thread(t) => self.hasher_word(6, u64::from(t.0)),
            Value::Nil | Value::Cell(_) => 0,
        }
    }

    fn hasher_word(&self, tag: u8, word: u64) -> u64 {
        self.hasher.word(tag, word)
    }

    /// `t[key]` without metamethods.
    #[inline]
    pub(crate) fn raw_get(&self, t: TableId, key: Value) -> Value {
        let table = self.tables.get(t.0);
        match key {
            Value::Number(n) => {
                if let Some(i) = crate::number::to_integer(n) {
                    return table.get_int(i, &self.hasher);
                }
                table.get(key, self.hasher.number(n))
            }
            Value::Nil => Value::Nil,
            _ => table.get(key, self.key_hash(key)),
        }
    }

    /// `t[key] = value` without metamethods; `key` is not nil or NaN (the caller checks).
    pub(crate) fn raw_set(&mut self, t: TableId, key: Value, value: Value) -> Result<(), LuaError> {
        let hash = self.key_hash(key);
        let hasher = self.hasher;
        let grown = self.tables.get_mut(t.0).set(key, hash, value, &hasher);
        if grown > 0 {
            self.charge(grown)?;
        }
        Ok(())
    }
}
