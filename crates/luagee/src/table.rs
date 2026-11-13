//! Lua tables: an array part for the keys 1..n and a hash part that iterates in the order keys first arrived.
//!
//! Iteration never depends on an address or a hash: `next` walks the array part, then the hash part's entries in
//! insertion order. Assigning `nil` leaves the entry in place (so `next` keeps working while a loop clears fields);
//! the entries are compacted when the hash part grows.

use crate::heap::KeyHasher;
use crate::{Limit, LuaError};

pub(crate) type WriteBudget<'a> = dyn FnMut(u64, usize) -> Result<(), LuaError> + 'a;
use crate::value::{TableId, VALUE_BYTES, Value};

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug)]
struct Entry {
    key: Value,
    value: Value,
    hash: u64,
}

const ENTRY_BYTES: usize = 56;
const _: () = assert!(std::mem::size_of::<Entry>() == ENTRY_BYTES);

/// A table.
#[derive(Debug, Default)]
pub(crate) struct Table {
    /// The values of the keys 1..=len; a `nil` inside is a hole.
    array: Vec<Value>,
    /// The other keys, in the order they arrived; a `nil` value is a removed key that keeps its place.
    entries: Vec<Entry>,
    /// Open addressing over `entries`: 0 is empty, else the entry's index + 1. A power of two, or empty.
    index: Vec<u32>,
    pub(crate) metatable: Option<TableId>,
}

/// Why `next` found no entry to return.
pub(crate) enum NextError {
    /// The key is not in the table.
    InvalidKey,
    /// The next entry lies past the empty slots the caller can pay to step over.
    Unaffordable,
}

impl Table {
    /// An empty table with room for `array` array items and `hash` other keys; it holds `planned_bytes(array, hash)`.
    pub(crate) fn with_capacity(array: usize, hash: usize) -> Table {
        #[cfg(test)]
        tests::note_storage_construction();
        let (entries, index) =
            Self::hash_capacity(hash).expect("a table's planned storage is addressable");
        Table {
            array: Vec::with_capacity(array),
            entries: Vec::with_capacity(entries),
            index: vec![0; index],
            metatable: None,
        }
    }

    /// The bytes `with_capacity(array, hash)` will hold, known before any of it is allocated.
    pub(crate) fn planned_bytes(array: usize, hash: usize) -> Option<usize> {
        let (entries, index) = Self::hash_capacity(hash)?;
        crate::heap::TABLE_SIZE
            .checked_add(array.checked_mul(VALUE_BYTES)?)?
            .checked_add(entries.checked_mul(ENTRY_BYTES)?)?
            .checked_add(index.checked_mul(4)?)
    }

    /// The entries and index slots reserved for `hash` keys: none for none, else at least four entries (as growing
    /// from empty gives) and an index at most half full.
    fn hash_capacity(hash: usize) -> Option<(usize, usize)> {
        if hash == 0 {
            return Some((0, 0));
        }
        let index = hash.checked_mul(2)?.checked_next_power_of_two()?.max(4);
        Some((hash.max(4), index))
    }

    /// The bytes this table holds, as the interpreter counts them.
    pub(crate) fn bytes(&self) -> usize {
        crate::heap::TABLE_SIZE
            + self.array.capacity() * VALUE_BYTES
            + self.entries.capacity() * ENTRY_BYTES
            + self.index.capacity() * 4
    }

    fn array_slot(&self, key: Value) -> Option<usize> {
        let Value::Number(n) = key else { return None };
        let i = crate::number::to_integer(n)?;
        (1..=self.array.len() as i64 + 1)
            .contains(&i)
            .then(|| i as usize - 1)
    }

    fn find(&self, key: Value, hash: u64) -> Option<usize> {
        if self.index.is_empty() {
            return None;
        }
        let mask = self.index.len() - 1;
        let mut slot = hash as usize & mask;
        loop {
            let entry = self.index[slot];
            if entry == 0 {
                return None;
            }
            let found = &self.entries[entry as usize - 1];
            if found.hash == hash && found.key == key {
                return Some(entry as usize - 1);
            }
            slot = (slot + 1) & mask;
        }
    }

    /// The value at `key` (whose hash is `hash`), without metamethods.
    #[inline]
    pub(crate) fn get(&self, key: Value, hash: u64) -> Value {
        if let Some(i) = self.array_slot(key)
            && i < self.array.len()
        {
            return self.array[i];
        }
        self.find(key, hash)
            .map_or(Value::Nil, |e| self.entries[e].value)
    }

    /// The value at the integer key `i`, without metamethods.
    #[inline]
    pub(crate) fn get_int(&self, i: i64, hasher: &KeyHasher) -> Value {
        if i >= 1 && (i as u64) <= self.array.len() as u64 {
            return self.array[i as usize - 1];
        }
        let key = Value::Number(i as f64);
        self.find(key, hasher.number(i as f64))
            .map_or(Value::Nil, |e| self.entries[e].value)
    }

    /// Writes a value, funding copying, migration, rehashing and allocation before structural changes.
    pub(crate) fn set(
        &mut self,
        key: Value,
        hash: impl FnOnce() -> u64,
        value: Value,
        hasher: &KeyHasher,
        budget: &mut WriteBudget<'_>,
    ) -> Result<(), LuaError> {
        if let Some(i) = self.array_slot(key) {
            if i < self.array.len() {
                self.array[i] = value;
                return Ok(());
            }
            if value == Value::Nil {
                debug_assert!(
                    self.find(key, hash())
                        .is_none_or(|entry| self.entries[entry].value == Value::Nil),
                    "the next array key has no live hash entry"
                );
                return Ok(());
            }
            return self.append(value, hasher, budget);
        }
        let hash = hash();
        if let Some(e) = self.find(key, hash) {
            self.entries[e].value = value;
            return Ok(());
        }
        if value == Value::Nil {
            return Ok(());
        }
        if self.entries.len().saturating_add(1).saturating_mul(2) > self.index.len() {
            self.rehash(budget)?;
        }
        self.reserve_entry(budget)?;
        let key = if key == Value::Number(0.0) {
            Value::Number(0.0)
        } else {
            key
        };
        self.entries.push(Entry { key, value, hash });
        self.place(self.entries.len() - 1);
        Ok(())
    }

    /// Rollback of an existing entry cannot allocate, migrate or rebuild, and must work with exhausted fuel.
    pub(crate) fn clear_existing(&mut self, key: Value, hash: u64) {
        if let Some(index) = self.array_slot(key)
            && index < self.array.len()
        {
            self.array[index] = Value::Nil;
        } else if let Some(index) = self.find(key, hash) {
            self.entries[index].value = Value::Nil;
        }
    }

    fn reserve_entry(&mut self, budget: &mut WriteBudget<'_>) -> Result<(), LuaError> {
        if self.entries.len() < self.entries.capacity() {
            return Ok(());
        }
        let growth = self.entries.capacity().max(4);
        let bytes = growth
            .checked_mul(ENTRY_BYTES)
            .ok_or(LuaError::Limit(Limit::Memory))?;
        budget(1 + self.entries.len() as u64 / 8, bytes)?;
        self.entries.reserve_exact(growth);
        Ok(())
    }

    fn reserve_array(
        &mut self,
        required: usize,
        budget: &mut WriteBudget<'_>,
    ) -> Result<(), LuaError> {
        if required <= self.array.capacity() {
            return Ok(());
        }
        let doubled = self
            .array
            .capacity()
            .checked_mul(2)
            .ok_or(LuaError::Limit(Limit::Memory))?;
        let rounded = required
            .checked_next_power_of_two()
            .ok_or(LuaError::Limit(Limit::Memory))?;
        let capacity = doubled.max(rounded).max(4);
        let bytes = (capacity - self.array.capacity())
            .checked_mul(VALUE_BYTES)
            .ok_or(LuaError::Limit(Limit::Memory))?;
        budget(1 + self.array.len() as u64 / 8, bytes)?;
        self.array.reserve_exact(capacity - self.array.len());
        Ok(())
    }

    /// Plans all migration before mutation: fuel exhaustion cannot leave a partial contiguous array prefix.
    fn migration_count(
        &self,
        hasher: &KeyHasher,
        budget: &mut WriteBudget<'_>,
    ) -> Result<usize, LuaError> {
        if self.entries.is_empty() {
            budget(1, 0)?;
            return Ok(0);
        }
        for count in 0..=self.entries.len() {
            budget(1, 0)?;
            let next = self
                .array
                .len()
                .checked_add(2)
                .and_then(|n| n.checked_add(count))
                .ok_or(LuaError::Limit(Limit::Memory))?;
            crate::work::visit(crate::work::Work::Steps, 1);
            match self.find(Value::Number(next as f64), hasher.number(next as f64)) {
                Some(e) if self.entries[e].value != Value::Nil => {}
                _ => return Ok(count),
            }
        }
        unreachable!("a migration run cannot contain more distinct keys than the hash part")
    }

    fn append(
        &mut self,
        value: Value,
        hasher: &KeyHasher,
        budget: &mut WriteBudget<'_>,
    ) -> Result<(), LuaError> {
        let count = self.migration_count(hasher, budget)?;
        // Moving a planned key looks it up again: paid before anything moves.
        budget(count as u64, 0)?;
        let required = self
            .array
            .len()
            .checked_add(1)
            .and_then(|n| n.checked_add(count))
            .ok_or(LuaError::Limit(Limit::Memory))?;
        self.reserve_array(required, budget)?;
        self.array.push(value);
        for _ in 0..count {
            crate::work::visit(crate::work::Work::Steps, 1);
            let next = (self.array.len() + 1) as f64;
            let entry = self
                .find(Value::Number(next), hasher.number(next))
                .expect("a planned live migration");
            self.array.push(self.entries[entry].value);
            self.entries[entry].value = Value::Nil;
        }
        Ok(())
    }

    fn rehash(&mut self, budget: &mut WriteBudget<'_>) -> Result<(), LuaError> {
        budget(1 + self.entries.len() as u64 / 8, 0)?;
        let live = self
            .entries
            .iter()
            .filter(|e| e.value != Value::Nil)
            .count();
        let size = live
            .checked_add(1)
            .and_then(|n| n.checked_mul(2))
            .and_then(usize::checked_next_power_of_two)
            .ok_or(LuaError::Limit(Limit::Memory))?
            .max(4);
        let growth = size.saturating_sub(self.index.capacity());
        let bytes = growth
            .checked_mul(4)
            .ok_or(LuaError::Limit(Limit::Memory))?;
        let work = self
            .entries
            .len()
            .saturating_add(size)
            .saturating_add(self.index.len())
            .saturating_add(live);
        budget(1 + work as u64 / 8, bytes)?;
        if growth > 0 {
            self.index.reserve_exact(size - self.index.len());
        }
        self.entries.retain(|e| e.value != Value::Nil);
        self.index.clear();
        self.index.resize(size, 0);
        for e in 0..self.entries.len() {
            self.place(e);
        }
        Ok(())
    }

    fn place(&mut self, entry: usize) {
        let mask = self.index.len() - 1;
        let mut slot = self.entries[entry].hash as usize & mask;
        while self.index[slot] != 0 {
            slot = (slot + 1) & mask;
        }
        self.index[slot] = entry as u32 + 1;
    }

    /// The length operator: a border (a non-nil `t[n]` with a nil `t[n + 1]`, or 0), found the same way every time.
    pub(crate) fn border(&self) -> usize {
        let n = self.array.len();
        if n == 0 || self.array[n - 1] != Value::Nil {
            return n;
        }
        let (mut low, mut high) = (0, n);
        while high - low > 1 {
            let middle = (low + high) / 2;
            if self.array[middle - 1] == Value::Nil {
                high = middle;
            } else {
                low = middle;
            }
        }
        low
    }

    /// The entry after `key` (nil: the first), for `next` and `pairs`, and how many empty slots it stepped over (the
    /// work the caller pays for), stepping over no more than `limit`.
    pub(crate) fn next(
        &self,
        key: Value,
        hash: u64,
        limit: usize,
    ) -> Result<(Option<(Value, Value)>, usize), NextError> {
        let mut at = match key {
            Value::Nil => 0,
            _ => match self.array_slot(key) {
                Some(i) if i < self.array.len() => i + 1,
                _ => {
                    let entry = self.find(key, hash).ok_or(NextError::InvalidKey)?;
                    self.array.len() + entry + 1
                }
            },
        };
        let start = at;
        let slots = self.array.len() + self.entries.len();
        while at < slots {
            let found = if at < self.array.len() {
                let value = self.array[at];
                (value != Value::Nil).then(|| (Value::Number((at + 1) as f64), value))
            } else {
                let entry = &self.entries[at - self.array.len()];
                (entry.value != Value::Nil).then_some((entry.key, entry.value))
            };
            if found.is_some() {
                return Ok((found, at - start));
            }
            if at - start == limit {
                return Err(NextError::Unaffordable);
            }
            crate::work::visit(crate::work::Work::Steps, 1);
            at += 1;
        }
        Ok((None, at - start))
    }

    /// Removes every key, keeping the storage (`table.clear`): as assigning nil to each, so a `next` loop over the
    /// table goes on.
    pub(crate) fn clear(&mut self) {
        self.array.fill(Value::Nil);
        for entry in &mut self.entries {
            entry.value = Value::Nil;
        }
    }

    /// How many slots the table holds, set or not: the work of clearing it.
    pub(crate) fn slots(&self) -> usize {
        self.array.len() + self.entries.len()
    }

    /// The array part's values, for `unpack`, `concat` and the collector.
    pub(crate) fn array(&self) -> &[Value] {
        &self.array
    }

    /// How many hash entries the table holds, removed ones too: the collector's walk over them.
    pub(crate) fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// A hash entry's key and value (nil for a removed key), for the collector.
    pub(crate) fn entry(&self, index: usize) -> (Value, Value) {
        let entry = &self.entries[index];
        (entry.key, entry.value)
    }

    /// The largest positive numeric key (`table.maxn`), found looking at no more than `limit` slots: the array's nil
    /// tail from its end, then every hash entry, removed or not. With the slots looked at; None when `limit` is short.
    pub(crate) fn max_integer_key(&self, limit: usize) -> Option<(f64, usize)> {
        let mut visited = 0;
        let mut most = 0.0;
        for (i, value) in self.array.iter().enumerate().rev() {
            if visited == limit {
                return None;
            }
            visited += 1;
            crate::work::visit(crate::work::Work::Slots, 1);
            if *value != Value::Nil {
                most = (i + 1) as f64;
                break;
            }
        }
        for e in &self.entries {
            if visited == limit {
                return None;
            }
            visited += 1;
            crate::work::visit(crate::work::Work::Slots, 1);
            if e.value != Value::Nil
                && let Value::Number(n) = e.key
                && n > most
            {
                most = n;
            }
        }
        Some((most, visited))
    }
}
