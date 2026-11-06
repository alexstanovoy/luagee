//! Lua tables: an array part for the keys 1..n and a hash part that iterates in the order keys first arrived.
//!
//! Iteration never depends on an address or a hash: `next` walks the array part, then the hash part's entries in
//! insertion order. Assigning `nil` leaves the entry in place (so `next` keeps working while a loop clears fields);
//! the entries are compacted when the hash part grows.

use crate::heap::KeyHasher;
use crate::value::{TableId, Value};

#[derive(Clone, Copy, Debug)]
struct Entry {
    key: Value,
    value: Value,
    hash: u64,
}

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

/// What `next` found wrong.
pub(crate) struct InvalidKey;

impl Table {
    /// An empty table with room for `array` array items and `hash` other keys.
    pub(crate) fn with_capacity(array: usize, hash: usize) -> Table {
        let mut table = Table {
            array: Vec::with_capacity(array),
            ..Table::default()
        };
        if hash > 0 {
            table.entries.reserve(hash);
            table.index = vec![0; (hash * 2).next_power_of_two().max(4)];
        }
        table
    }

    /// The bytes this table holds, as the interpreter counts them.
    pub(crate) fn bytes(&self) -> usize {
        crate::heap::TABLE_SIZE
            + self.array.capacity() * 16
            + self.entries.capacity() * 40
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

    /// Sets `key` (not nil, not NaN; whose hash is `hash`) to `value`, without metamethods. Returns the bytes the
    /// table grew by.
    pub(crate) fn set(&mut self, key: Value, hash: u64, value: Value, hasher: &KeyHasher) -> usize {
        let before = self.bytes();
        if let Some(i) = self.array_slot(key) {
            if i < self.array.len() {
                self.array[i] = value;
                return 0;
            }
            if value == Value::Nil {
                if let Some(e) = self.find(key, hash) {
                    self.entries[e].value = Value::Nil;
                }
                return 0;
            }
            // Appending: the hash part never holds the key after the array part, so this key is new.
            self.array.push(value);
            self.migrate(hasher);
            return self.bytes().saturating_sub(before);
        }
        if let Some(e) = self.find(key, hash) {
            self.entries[e].value = value;
            return 0;
        }
        if value == Value::Nil {
            return 0;
        }
        if (self.entries.len() + 1) * 2 > self.index.len() {
            self.rehash();
        }
        // -0 and 0 are one key; it is stored as 0.
        let key = if key == Value::Number(0.0) {
            Value::Number(0.0)
        } else {
            key
        };
        self.entries.push(Entry { key, value, hash });
        self.place(self.entries.len() - 1);
        self.bytes().saturating_sub(before)
    }

    /// Moves the keys that now follow the array part from the hash part into it.
    fn migrate(&mut self, hasher: &KeyHasher) {
        loop {
            let next = (self.array.len() + 1) as f64;
            let key = Value::Number(next);
            match self.find(key, hasher.number(next)) {
                Some(e) if self.entries[e].value != Value::Nil => {
                    self.array.push(self.entries[e].value);
                    self.entries[e].value = Value::Nil;
                }
                _ => return,
            }
        }
    }

    fn rehash(&mut self) {
        self.entries.retain(|e| e.value != Value::Nil);
        let size = ((self.entries.len() + 1) * 2).next_power_of_two().max(4);
        self.index.clear();
        self.index.resize(size, 0);
        for e in 0..self.entries.len() {
            self.place(e);
        }
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
    /// work the caller pays for).
    pub(crate) fn next(
        &self,
        key: Value,
        hash: u64,
    ) -> Result<(Option<(Value, Value)>, usize), InvalidKey> {
        let mut at = match key {
            Value::Nil => 0,
            _ => match self.array_slot(key) {
                Some(i) if i < self.array.len() => i + 1,
                _ => self.array.len() + self.find(key, hash).ok_or(InvalidKey)? + 1,
            },
        };
        let start = at;
        while at < self.array.len() {
            if self.array[at] != Value::Nil {
                return Ok((
                    Some((Value::Number((at + 1) as f64), self.array[at])),
                    at - start,
                ));
            }
            at += 1;
        }
        let rest = &self.entries[at - self.array.len()..];
        match rest.iter().position(|e| e.value != Value::Nil) {
            Some(i) => Ok((Some((rest[i].key, rest[i].value)), at - start + i)),
            None => Ok((None, at - start + rest.len())),
        }
    }

    /// The array part's values, for `unpack`, `concat` and the collector.
    pub(crate) fn array(&self) -> &[Value] {
        &self.array
    }

    /// Every live key and value, for the collector.
    pub(crate) fn pairs(&self) -> impl Iterator<Item = (Value, Value)> + '_ {
        self.entries
            .iter()
            .filter(|e| e.value != Value::Nil)
            .map(|e| (e.key, e.value))
    }

    /// The largest positive integer key (`table.maxn`).
    pub(crate) fn max_integer_key(&self) -> f64 {
        let mut most = self
            .array
            .iter()
            .rposition(|v| *v != Value::Nil)
            .map_or(0.0, |i| (i + 1) as f64);
        for e in self.entries.iter().filter(|e| e.value != Value::Nil) {
            if let Value::Number(n) = e.key
                && n > most
            {
                most = n;
            }
        }
        most
    }
}
