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

#[cfg(test)]
mod tests;

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
    pub(crate) fn new(key: [u64; 2]) -> KeyHasher {
        KeyHasher {
            k0: key[0],
            k1: key[1],
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
    generation: u32,
}

/// The size of an arena slot holding a `T` on this target.
pub(crate) const fn slot_size<T>() -> usize {
    std::mem::size_of::<Slot<T>>()
}

/// Objects of one kind, addressed by index.
#[derive(Debug)]
pub(crate) struct Arena<T> {
    slots: Vec<Slot<T>>,
    free: Vec<u32>,
    /// The objects held.
    objects: usize,
    /// How far a collection's sweep got: an object made below it is born unmarked, at or above it (all of the arena
    /// while marking, or before the sweep reaches the arena) born marked, so that the sweep spares it. `usize::MAX`
    /// when no collection runs.
    swept: usize,
}

impl<T> Default for Arena<T> {
    fn default() -> Self {
        Arena {
            slots: Vec::new(),
            free: Vec::new(),
            objects: 0,
            swept: usize::MAX,
        }
    }
}

impl<T> Arena<T> {
    /// Stores an object.
    pub(crate) fn insert(&mut self, object: T) -> u64 {
        self.objects += 1;
        let i = match self.free.pop() {
            Some(i) => i as usize,
            None => {
                self.slots.push(Slot {
                    object: None,
                    marked: false,
                    generation: 0,
                });
                self.slots.len() - 1
            }
        };
        let generation = self.slots[i].generation + 1;
        self.slots[i] = Slot {
            object: Some(object),
            marked: i >= self.swept,
            generation,
        };
        let index = u32::try_from(i).expect("an arena holds fewer than 2^32 objects");
        u64::from(generation) << 32 | u64::from(index)
    }

    /// The slots the next `insert` adds to the arena's storage (it doubles when full): what to charge before it.
    pub(crate) fn growth(&self) -> usize {
        if self.free.is_empty() && self.slots.len() == self.slots.capacity() {
            self.slots.capacity().max(4)
        } else {
            0
        }
    }

    /// A live handle at a trusted slot index (the intern index is maintained with the arena).
    pub(crate) fn id_at(&self, index: u32) -> u64 {
        let slot = &self.slots[index as usize];
        u64::from(slot.generation) << 32 | u64::from(index)
    }

    pub(crate) fn try_get(&self, id: u64) -> Option<&T> {
        let slot = self.slots.get(id as u32 as usize)?;
        (slot.generation == (id >> 32) as u32).then_some(())?;
        slot.object.as_ref()
    }

    pub(crate) fn try_get_mut(&mut self, id: u64) -> Option<&mut T> {
        let slot = self.slots.get_mut(id as u32 as usize)?;
        (slot.generation == (id >> 32) as u32).then_some(())?;
        slot.object.as_mut()
    }

    #[inline]
    pub(crate) fn get(&self, id: u64) -> &T {
        self.try_get(id).expect("a live internal handle")
    }

    #[inline]
    pub(crate) fn get_mut(&mut self, id: u64) -> &mut T {
        self.try_get_mut(id).expect("a live internal handle")
    }

    /// Marks an internal live handle; true when it was not marked yet.
    pub(crate) fn mark(&mut self, id: u64) -> bool {
        let slot = &mut self.slots[id as u32 as usize];
        debug_assert_eq!(slot.generation, (id >> 32) as u32);
        !std::mem::replace(&mut slot.marked, true)
    }

    /// Revives an interned string without changing its generation.
    pub(crate) fn keep(&mut self, id: u64) {
        let index = id as u32 as usize;
        if index >= self.swept {
            self.slots[index].marked = true;
        }
    }

    /// A collection starts: from now on, objects are born marked until the sweep passes them.
    pub(crate) fn begin_collection(&mut self) {
        self.swept = 0;
    }

    /// Sweeps up to `budget` slots on from where the sweep got: frees each unmarked object (handing it to `freed`)
    /// and unmarks the rest. Returns the counted bytes (by `size`) of the objects kept, and the slots walked.
    pub(crate) fn sweep(
        &mut self,
        budget: usize,
        size: impl Fn(&T) -> usize,
        mut freed: impl FnMut(u32, T),
    ) -> (usize, usize) {
        let start = self.swept;
        assert!(
            start <= self.slots.len(),
            "a sweep runs only during a collection"
        );
        let end = start.saturating_add(budget).min(self.slots.len());
        let mut kept = 0;
        for i in start..end {
            let slot = &mut self.slots[i];
            if std::mem::replace(&mut slot.marked, false) {
                kept += slot.object.as_ref().map_or(0, &size);
            } else if let Some(object) = slot.object.take() {
                self.objects -= 1;
                // Retire exhausted generations; a stale handle must never become valid again.
                if slot.generation < u32::MAX { self.free.push(i as u32); }
                freed(i as u32, object);
            }
        }
        self.swept = end;
        (kept, end - start)
    }

    /// Whether the sweep has walked every slot.
    pub(crate) fn is_swept(&self) -> bool {
        self.swept >= self.slots.len()
    }

    /// Ends the arena's sweep; returns the counted bytes of the arena's own storage: `empty_slot` for each slot that
    /// holds nothing, and the free list.
    ///
    /// The slot size is a counted constant rather than `size_of`, so that the count is the same on every platform.
    pub(crate) fn end_sweep(&mut self, empty_slot: usize) -> usize {
        self.swept = usize::MAX;
        (self.slots.capacity() - self.objects) * empty_slot + self.free.capacity() * 4
    }
}

/// Where a string's run in the index starts.
#[inline]
fn home(hash: u64) -> usize {
    (hash & 0xffff_ffff) as usize
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
    pub(crate) bytes: usize,
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
    /// Open addressing over `strings` by content, linear probing: 0 is empty, else the low 32 bits of the string's
    /// hash above its id + 1 (the hash, so that an entry can move without looking at its string).
    intern: Vec<u64>,
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
    /// Where the collector is.
    pub(crate) phase: Phase,
    /// The collector's worklist: values reached but not traced yet.
    pub(crate) gray: Vec<Value>,
}

/// Where the collector is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Phase {
    /// No collection runs.
    Idle,
    /// Tracing from the roots: stores into objects gray what they store, and new objects gray what they refer to.
    Marking,
    /// Freeing what the marking did not reach, a few slots at a time.
    Sweeping,
}

/// The counted size of an entry of the string index.
pub(crate) const INTERN_ENTRY: usize = 8;

/// The counted size of a string object beyond its bytes.
pub(crate) const STRING_OVERHEAD: usize = 32;
/// The counted size of a function object.
pub(crate) const FUNCTION_SIZE: usize = 64;
/// The counted size of a cell.
pub(crate) const CELL_SIZE: usize = 24;
/// The counted size of a userdata object (its host data is the host's).
pub(crate) const USERDATA_SIZE: usize = 48;
/// The counted size of a table object beyond its parts.
pub(crate) const TABLE_SIZE: usize = 96;

// An object's counted size covers its arena slot on 64-bit targets, the largest, so an empty slot counts as much as
// the object it held.
#[cfg(target_pointer_width = "64")]
const _: () = {
    assert!(std::mem::size_of::<Slot<LuaString>>() <= STRING_OVERHEAD);
    assert!(std::mem::size_of::<Slot<Function>>() <= FUNCTION_SIZE);
    assert!(std::mem::size_of::<Slot<Value>>() <= CELL_SIZE);
    assert!(std::mem::size_of::<Slot<Userdata>>() <= USERDATA_SIZE);
    assert!(std::mem::size_of::<Slot<Table>>() <= TABLE_SIZE);
};

impl Heap {
    pub(crate) fn new(cap: usize, hash_key: [u64; 2]) -> Heap {
        Heap {
            strings: Arena::default(),
            intern: vec![0; 1024],
            interned: 0,
            tables: Arena::default(),
            functions: Arena::default(),
            userdata: Arena::default(),
            cells: Arena::default(),
            hasher: KeyHasher::new(hash_key),
            memory: 0,
            cap,
            phase: Phase::Idle,
            gray: Vec::new(),
        }
    }

    /// The hard ceiling: past the cap by a quarter, so that a native function's allocations between two of the
    /// collector's points (where the cap itself is checked, after collecting) cannot fail just short of reclaiming.
    pub(crate) fn ceiling(&self) -> usize {
        self.cap.saturating_add(self.cap / 4)
    }

    /// The write barrier: while marking, a value stored into an object is grayed, so that the object, if already
    /// traced, does not hide it from the collection.
    #[inline]
    pub(crate) fn barrier(&mut self, value: Value) {
        if self.phase == Phase::Marking
            && !matches!(value, Value::Nil | Value::Boolean(_) | Value::Number(_))
        {
            self.gray.push(value);
        }
    }

    /// Counts `bytes` more, failing when that crosses the ceiling.
    #[inline]
    pub(crate) fn charge(&mut self, bytes: usize) -> Result<(), LuaError> {
        let total = self.memory.saturating_add(bytes);
        if total > self.ceiling() {
            return Err(LuaError::Limit(crate::Limit::Memory));
        }
        self.memory = total;
        Ok(())
    }

    /// The interned string with these bytes, made if needed.
    pub(crate) fn intern(&mut self, bytes: &[u8]) -> Result<StrId, LuaError> {
        let hash = self.hasher.bytes(bytes);
        let mask = self.intern.len() - 1;
        let mut slot = home(hash) & mask;
        while self.intern[slot] != 0 {
            let entry = self.intern[slot];
            if entry >> 32 == hash & 0xffff_ffff {
                let id = self.strings.id_at(entry as u32 - 1);
                let s = self.strings.get(id);
                if s.hash == hash && *s.bytes == *bytes {
                    self.strings.keep(id);
                    return Ok(StrId(id));
                }
            }
            slot = (slot + 1) & mask;
        }
        let grow = (self.interned + 1) * 2 > self.intern.len();
        let index_growth = if grow {
            self.intern.len() * INTERN_ENTRY
        } else {
            0
        };
        self.charge(
            STRING_OVERHEAD + bytes.len() + self.strings.growth() * STRING_OVERHEAD + index_growth,
        )?;
        if grow {
            self.rehash_intern(self.intern.len() * 2);
        }
        let id = self.strings.insert(LuaString {
            bytes: bytes.into(),
            hash,
        });
        let mask = self.intern.len() - 1;
        let mut slot = home(hash) & mask;
        while self.intern[slot] != 0 {
            slot = (slot + 1) & mask;
        }
        self.intern[slot] = (hash & 0xffff_ffff) << 32 | u64::from(id as u32 + 1);
        self.interned += 1;
        Ok(StrId(id))
    }

    /// Moves the string index to `size` entries (a power of two, more than twice the strings), from its own entries.
    fn rehash_intern(&mut self, size: usize) {
        assert!(size.is_power_of_two(), "the string index is a power of two");
        assert!(
            size > self.interned * 2,
            "the string index stays at most half full"
        );
        assert!(
            size as u64 <= 1 << 32,
            "the string index is addressed by 32 bits of hash"
        );
        let old = std::mem::replace(&mut self.intern, vec![0; size]);
        let mask = size - 1;
        for entry in old.into_iter().filter(|&entry| entry != 0) {
            let mut slot = (entry >> 32) as usize & mask;
            while self.intern[slot] != 0 {
                slot = (slot + 1) & mask;
            }
            self.intern[slot] = entry;
        }
    }

    /// Takes a freed string out of the index, moving later entries of its run back into the gap (so that no lookup
    /// stops early at it).
    fn unintern(intern: &mut [u64], interned: &mut usize, id: u32, hash: u64) {
        let mask = intern.len() - 1;
        let target = (hash & 0xffff_ffff) << 32 | u64::from(id + 1);
        let mut hole = home(hash) & mask;
        while intern[hole] != target {
            assert_ne!(intern[hole], 0, "a freed string was in the index");
            hole = (hole + 1) & mask;
        }
        let mut next = (hole + 1) & mask;
        while intern[next] != 0 {
            let entry_home = (intern[next] >> 32) as usize & mask;
            // The entry stays when its home lies cyclically in (hole, next]: a lookup from there never passes the hole.
            let stays = if hole < next {
                hole < entry_home && entry_home <= next
            } else {
                hole < entry_home || entry_home <= next
            };
            if !stays {
                intern[hole] = intern[next];
                hole = next;
            }
            next = (next + 1) & mask;
        }
        intern[hole] = 0;
        *interned -= 1;
    }

    /// Sweeps up to `budget` string slots, taking the freed strings out of the index; as `Arena::sweep`.
    pub(crate) fn sweep_strings(&mut self, budget: usize) -> (usize, usize) {
        let Heap {
            strings,
            intern,
            interned,
            ..
        } = self;
        strings.sweep(
            budget,
            |s| STRING_OVERHEAD + s.bytes.len(),
            |id, s| Heap::unintern(intern, interned, id, s.hash),
        )
    }

    /// The string index's entries.
    pub(crate) fn intern_size(&self) -> usize {
        self.intern.len()
    }

    /// Shrinks the string index after a sweep left it mostly empty; false when it is dense enough already.
    pub(crate) fn shrink_intern(&mut self) -> bool {
        let size = (self.interned * 4).next_power_of_two().max(1024);
        if size * 2 > self.intern.len() {
            return false;
        }
        self.rehash_intern(size);
        true
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

    /// Sets a table's metatable (behind the barrier).
    pub(crate) fn set_metatable(&mut self, id: TableId, metatable: Option<TableId>) {
        if let Some(mt) = metatable {
            self.barrier(Value::Table(mt));
        }
        self.tables.get_mut(id.0).metatable = metatable;
    }

    /// Sets a cell's value (behind the barrier).
    #[inline]
    pub(crate) fn set_cell(&mut self, cell: CellId, value: Value) {
        self.barrier(value);
        *self.cells.get_mut(cell.0) = value;
    }

    pub(crate) fn new_table(&mut self, array: usize, hash: usize) -> Result<TableId, LuaError> {
        let table = Table::with_capacity(array, hash);
        self.charge(table.bytes() + self.tables.growth() * TABLE_SIZE)?;
        Ok(TableId(self.tables.insert(table)))
    }

    pub(crate) fn new_function(&mut self, function: Function) -> Result<FuncId, LuaError> {
        let size = FUNCTION_SIZE
            + match &function {
                Function::Lua { upvalues, .. } => upvalues.len() * 8,
                Function::Native { upvalues, .. } => upvalues.len() * 16,
            };
        self.charge(size + self.functions.growth() * FUNCTION_SIZE)?;
        // Born marked during a collection, so what it refers to is grayed here: nothing else would trace it.
        if self.phase == Phase::Marking {
            match &function {
                Function::Lua { upvalues, env, .. } => {
                    self.gray.extend(upvalues.iter().map(|c| Value::Cell(*c)));
                    self.gray.push(Value::Table(*env));
                }
                Function::Native { upvalues, .. } => self.gray.extend_from_slice(upvalues),
            }
        }
        Ok(FuncId(self.functions.insert(function)))
    }

    pub(crate) fn new_cell(&mut self, value: Value) -> Result<CellId, LuaError> {
        self.charge(CELL_SIZE + self.cells.growth() * CELL_SIZE)?;
        self.barrier(value);
        Ok(CellId(self.cells.insert(value)))
    }

    pub(crate) fn new_userdata(
        &mut self,
        data: Box<dyn Any>,
        metatable: Option<TableId>,
    ) -> Result<UserdataId, LuaError> {
        let bytes = std::mem::size_of_val(data.as_ref());
        self.new_userdata_charged(data, metatable, bytes)
    }

    pub(crate) fn new_userdata_charged(
        &mut self,
        data: Box<dyn Any>,
        metatable: Option<TableId>,
        bytes: usize,
    ) -> Result<UserdataId, LuaError> {
        let size = bytes
            .checked_add(USERDATA_SIZE + self.userdata.growth() * USERDATA_SIZE)
            .ok_or(LuaError::Limit(crate::Limit::Memory))?;
        self.charge(size)?;
        if let Some(mt) = metatable {
            self.barrier(Value::Table(mt));
        }
        Ok(UserdataId(self.userdata.insert(Userdata {
            data,
            metatable,
            bytes,
        })))
    }

    /// The hash of a table key (not nil, not NaN).
    #[inline]
    pub(crate) fn key_hash(&self, key: Value) -> u64 {
        match key {
            Value::String(s) => self.strings.get(s.0).hash,
            Value::Number(n) => self.hasher.number(n),
            Value::Boolean(b) => self.hasher_word(2, u64::from(b)),
            Value::Table(t) => self.hasher_word(3, u64::from(t.0 as u32)),
            Value::Function(f) => self.hasher_word(4, u64::from(f.0 as u32)),
            Value::Userdata(u) => self.hasher_word(5, u64::from(u.0 as u32)),
            Value::Thread(t) => self.hasher_word(6, u64::from(t.0 as u32)),
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
        self.barrier(key);
        self.barrier(value);
        let grown = self.tables.get_mut(t.0).set(key, hash, value, &hasher);
        if grown > 0 {
            self.charge(grown)?;
        }
        Ok(())
    }
}
